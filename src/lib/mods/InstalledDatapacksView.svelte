<script lang="ts">
  // The instance-level datapack library screen (slice-2 design §7.4): every
  // pack Lucerna knows about — the registry UNION packs still linked in
  // worlds — each with its per-world state, expandable into per-world rows.
  //
  // Deliberate differences from InstalledAssetsView, its closest sibling:
  //   * The row leads with WORLD STATE («Включён в 2 из 3 мирах»); «Ни в
  //     одном мире» is an accent state, not an absent value — it is the state
  //     this screen exists to surface.
  //   * pack_format compatibility renders ONCE on the collapsed row — the
  //     verdict is per-instance, so a per-world copy would print N duplicates.
  //   * Removal routes through the shared cascade dialog, never a direct call.
  //   * No filters and no sorting, by design (§7.4).
  import {
    commands,
    type AssetUpdateState,
    type DatapackLibraryEntry,
    type DatapackLibraryView,
    type LoaderKind,
    type ModSource,
    type ModSummary,
    type ModVersion,
    type WorldPackState,
  } from '$lib/ipc/bindings';
  import { events } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { listenUntilDestroyed } from '$lib/ipc/listen';
  import { datapacksChanged } from '$lib/settings/state.svelte';
  import { datapackWorldSummary, datapacksDisabledKey } from '$lib/worlds/datapacks-gating';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import HelpPopover from '$lib/ui/HelpPopover.svelte';
  import { explanationState } from '$lib/onboarding/explanation-level.svelte';
  import { explainKey } from '$lib/onboarding/explanation-keys';
  import { get } from 'svelte/store';
  import { SvelteSet } from 'svelte/reactivity';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import CardShell from '$lib/ui/cards/CardShell.svelte';
  import CardMedia from '$lib/ui/cards/CardMedia.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import type { BadgeVariant } from '$lib/ui/cards/card-status';
  import ModDetailModal from './ModDetailModal.svelte';
  import ChangelogModal from './ChangelogModal.svelte';
  import { changelogSupported } from './changelog-supported';
  import DatapackWorldPicker from './DatapackWorldPicker.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import VanillaTweaksBuilder from '$lib/vanillatweaks/VanillaTweaksBuilder.svelte';
  import { installedVtPacks } from '$lib/vanillatweaks/vt-selection';
  import DatapackRemoveDialog from './DatapackRemoveDialog.svelte';

  let {
    instanceId,
    mcVersion = null,
    loader = null,
  }: {
    instanceId: string | null;
    mcVersion?: string | null;
    loader?: LoaderKind | null;
  } = $props();

  // "What are data packs?" — the same four-paragraph concept explainer shown on
  // all three datapack surfaces (this library, WorldDatapacks, and the server
  // pane), so a user meets the concept wherever they first land. Each paragraph
  // goes through explainKey, which swaps in the `*Basic` sibling at the Basic
  // detail level; the cast mirrors explainKey's own contract (a parity test
  // pins every pN/pNBasic pair in both locales).
  const datapackConceptParas = $derived(
    ['p1', 'p2', 'p3', 'p4'].map((p) =>
      $t(explainKey(`onboarding.datapackConcept.${p}` as TranslationKey, explanationState.level)),
    ),
  );

  let view = $state<DatapackLibraryView | null>(null);
  let loading = $state(false);
  let busy = $state(false);
  let checking = $state(false);
  let error = $state<string | null>(null);
  const expanded = new SvelteSet<string>();
  let summaries = $state<Map<string, ModSummary>>(new Map());
  let updateStates = $state<Map<string, AssetUpdateState>>(new Map());
  const updateCount = $derived(
    [...updateStates.values()].filter((s) => s.kind === 'update_available').length,
  );

  let detail = $state<{ source: ModSource; projectId: string; versionId: string | null } | null>(
    null,
  );
  let detailFor = $state<string | null>(null); // filename behind the open detail modal
  let pickerFor = $state<DatapackLibraryEntry | null>(null);
  let vtOpen = $state(false);
  let vtBusy = $state(false);
  // Which Vanilla Tweaks packs this library already holds, keyed by pack id.
  // Derived from the registry rows rather than stored: the registry is the
  // record of what is installed, so the builder never keeps a selection.
  const vtInstalled = $derived(
    installedVtPacks(
      (view?.entries ?? []).map((e) => ({
        source: e.pack.source,
        project_id: e.pack.project_id,
        version_id: e.pack.version_id,
      })),
    ),
  );

  // Build the selection, install it into the library, then report honestly:
  // a partly-failed build must not read as a success.
  async function buildVt(selection: [string, string[]][]) {
    if (!instanceId) return;
    vtBusy = true;
    try {
      const res = await commands.vtInstallToInstance(instanceId, selection);
      if (res.status === 'error') {
        error = formatError(res.error);
        return;
      }
      const failed = res.data.outcomes.filter((o) => !o.installed).length;
      if (failed > 0) {
        error = $t('addons.datapacks.vt.someFailed', { count: failed });
      } else {
        error = null;
        vtOpen = false;
      }
      await refresh();
      datapacksChanged.value++;
    } finally {
      vtBusy = false;
    }
  }
  let removeFor = $state<DatapackLibraryEntry | null>(null);
  let changelogReq = $state<{
    source: ModSource;
    projectId: string;
    title: string;
    target: string;
    base: string | null;
  } | null>(null);

  // Every datapack mutation is hard-guarded in the backend while the game
  // runs or starts; this is the matching UI gate (§7.7 — mutations reuse the
  // shared `datapacksDisabledKey`), so the buttons explain themselves instead
  // of failing with an error toast. Seeded from runningInstances and kept
  // live by the process events, mirroring RunningInstancesPopover.
  let running = $state(false);
  $effect(() => {
    const id = instanceId;
    if (id === null) {
      running = false;
      return;
    }
    void (async () => {
      try {
        const rows = await commands.runningInstances();
        if (instanceId !== id) return;
        running = rows.some((r) => r.instance_id === id);
      } catch {
        // Unknown ⟹ leave the UI usable; the backend guard is authoritative.
      }
    })();
  });
  listenUntilDestroyed([
    events.processSpawned.listen((e) => {
      if (e.payload.instance_id === instanceId) running = true;
    }),
    events.processExited.listen((e) => {
      if (e.payload.instance_id === instanceId) running = false;
    }),
  ]);
  const disabledKey = $derived(datapacksDisabledKey({ running, busy }));
  const gated = $derived(busy || disabledKey !== null);

  // Refetch on instance change and whenever a producer bumps datapacksChanged
  // (installs from Browse / the dropzone / this view's own actions). The
  // generation counter drops a stale in-flight response.
  //
  // Update badges and icon summaries are cleared only on a REAL instance
  // switch: a datapacksChanged bump also lands here after a FAILED update
  // (completed:false keeps the old version installed), and wiping
  // updateStates then would destroy the very update badge the user needs to
  // retry — the failure would become silent staleness.
  let generation = 0;
  let lastFetchedId: string | null = null;
  $effect(() => {
    const id = instanceId;
    void datapacksChanged.value;
    const gen = ++generation;
    if (id !== lastFetchedId) {
      updateStates = new Map();
      summaries = new Map();
    }
    lastFetchedId = id;
    if (id === null) {
      view = null;
      loading = false;
      error = null;
      return;
    }
    loading = true;
    error = null;
    void (async () => {
      const res = await commands.datapacksListLibrary(id);
      if (gen !== generation) return;
      if (res.status === 'error') {
        error = formatError(res.error);
        view = null;
      } else {
        view = res.data;
        void enrich(res.data, gen);
      }
      loading = false;
    })();
  });

  // Project icons for catalog-installed packs — same batched modsProjects
  // shape as InstalledAssetsView; hand-installed packs keep the placeholder.
  async function enrich(v: DatapackLibraryView, gen: number) {
    const idsBySource = new Map<ModSource, Set<string>>();
    for (const e of v.entries) {
      if (e.pack.source && e.pack.project_id) {
        const set = idsBySource.get(e.pack.source) ?? new Set<string>();
        set.add(e.pack.project_id);
        idsBySource.set(e.pack.source, set);
      }
    }
    const byProject = new Map<string, ModSummary>();
    await Promise.all(
      [...idsBySource].map(async ([src, ids]) => {
        const res = await commands.modsProjects(src, [...ids]);
        if (res.status === 'ok') {
          for (const s of res.data) byProject.set(`${src}:${s.project_id}`, s);
        }
      }),
    );
    if (gen !== generation) return;
    const map = new Map<string, ModSummary>();
    for (const e of v.entries) {
      if (!e.pack.source || !e.pack.project_id) continue;
      const s = byProject.get(`${e.pack.source}:${e.pack.project_id}`);
      if (s) map.set(e.pack.filename, s);
    }
    summaries = map;
  }

  async function refresh() {
    if (instanceId === null) return;
    const gen = ++generation;
    const res = await commands.datapacksListLibrary(instanceId);
    if (gen !== generation) return;
    if (res.status === 'error') error = formatError(res.error);
    else {
      view = res.data;
      void enrich(res.data, gen);
    }
  }

  async function checkUpdates() {
    if (instanceId === null) return;
    // Capture the instance: a slow network check started on instance A must
    // not land its badges on instance B's rows after a switch.
    const reqId = instanceId;
    checking = true;
    error = null;
    try {
      const res = await commands.datapacksCheckUpdates(reqId);
      if (instanceId !== reqId) return;
      if (res.status === 'error') {
        pushWarning(get(t)('addons.installed.checkFailed'), [formatError(res.error)]);
        return;
      }
      const map = new Map<string, AssetUpdateState>();
      for (const check of res.data) map.set(check.filename, check.state);
      updateStates = map;
      if (!res.data.some((c) => c.state.kind === 'update_available'))
        pushSuccess(get(t)('addons.installed.upToDateToast'));
    } finally {
      checking = false;
    }
  }

  // One update, with the per-world report the backend returns. `completed:
  // false` means at least one world failed and the OLD library file was kept
  // for a retry — the user must hear which worlds, or the failure is silent
  // staleness.
  async function update(entry: DatapackLibraryEntry, latest: ModVersion) {
    if (instanceId === null) return;
    busy = true;
    error = null;
    try {
      const res = await commands.datapacksUpdateOne(instanceId, entry.pack.filename, latest);
      if (res.status === 'error') {
        pushWarning(get(t)('addons.installed.updateFailedToast'), [formatError(res.error)]);
        return;
      }
      const failed = res.data.migrations.filter((m) => m.kind === 'failed');
      if (!res.data.completed && failed.length > 0) {
        pushWarning(
          get(t)('addons.datapacks.updateIncomplete', { count: failed.length }),
          failed.map((m) => (m.kind === 'failed' ? `${m.world}: ${m.details}` : m.kind)),
        );
      } else {
        pushSuccess(get(t)('addons.installed.updatedToast', { name: entry.pack.name }));
        const next = new Map(updateStates);
        next.delete(entry.pack.filename);
        updateStates = next;
      }
      await refresh();
      datapacksChanged.value++;
    } finally {
      busy = false;
    }
  }

  async function updateAll() {
    if (instanceId === null || view === null) return;
    const targets: Array<{ entry: DatapackLibraryEntry; latest: ModVersion }> = [];
    for (const e of view.entries) {
      const s = updateStates.get(e.pack.filename);
      if (s?.kind === 'update_available') targets.push({ entry: e, latest: s.latest });
    }
    if (targets.length === 0) return;
    busy = true;
    let updated = 0;
    let failed = 0;
    try {
      for (const tgt of targets) {
        const res = await commands.datapacksUpdateOne(
          instanceId,
          tgt.entry.pack.filename,
          tgt.latest,
        );
        if (res.status === 'error' || !res.data.completed) {
          failed++;
          continue;
        }
        updated++;
        const next = new Map(updateStates);
        next.delete(tgt.entry.pack.filename);
        updateStates = next;
      }
      await refresh();
      datapacksChanged.value++;
      if (failed === 0) pushSuccess(get(t)('addons.installed.toastUpdated', { count: updated }));
      else
        pushWarning(get(t)('addons.installed.toastUpdatedFailed', { count: updated, failed }), []);
    } finally {
      busy = false;
    }
  }

  async function toggleInWorld(entry: DatapackLibraryEntry, world: string, state: WorldPackState) {
    if (instanceId === null) return;
    busy = true;
    error = null;
    try {
      const res = await commands.datapacksSetEnabledInWorld(
        instanceId,
        world,
        entry.pack.filename,
        state !== 'enabled',
      );
      if (res.status === 'error') error = formatError(res.error);
      await refresh();
    } finally {
      busy = false;
    }
  }

  // Also the repair path for an orphaned sub-row: the backend clears the
  // level.dat name even when the file is already gone.
  async function removeFromWorld(entry: DatapackLibraryEntry, world: string) {
    if (instanceId === null) return;
    busy = true;
    error = null;
    try {
      const res = await commands.datapacksRemoveFromWorld(instanceId, world, entry.pack.filename);
      if (res.status === 'error') error = formatError(res.error);
      await refresh();
      datapacksChanged.value++;
    } finally {
      busy = false;
    }
  }

  function openDetail(entry: DatapackLibraryEntry) {
    if (!entry.pack.source || !entry.pack.project_id) return;
    detail = {
      source: entry.pack.source,
      projectId: entry.pack.project_id,
      versionId: entry.pack.version_id,
    };
    detailFor = entry.pack.filename;
  }

  // Installing a picked version over an existing library pack IS the update
  // path — datapacks_update_one owns replace semantics and world propagation,
  // so the detail modal's install routes there, not through a bare install.
  async function installDetailVersion(v: ModVersion) {
    if (instanceId === null || detailFor === null) return;
    const filename = detailFor;
    detail = null;
    detailFor = null;
    const entry = view?.entries.find((e) => e.pack.filename === filename);
    if (!entry) return;
    await update(entry, v);
  }

  function openAssetChangelog(entry: DatapackLibraryEntry, latest: ModVersion) {
    if (!entry.pack.source || !entry.pack.project_id) return;
    changelogReq = {
      source: entry.pack.source,
      projectId: entry.pack.project_id,
      title: `${entry.pack.name} ${entry.pack.version_number ?? ''} → ${latest.version_number}`,
      target: latest.version_id,
      base: entry.pack.version_id,
    };
  }

  function updatable(filename: string): ModVersion | null {
    const s = updateStates.get(filename);
    return s && s.kind === 'update_available' ? s.latest : null;
  }
  function checkFailedReason(filename: string): string | null {
    const s = updateStates.get(filename);
    return s?.kind === 'check_failed' ? s.reason : null;
  }

  function summaryVariant(emphasis: 'accent' | 'muted' | 'normal', enabled: number): BadgeVariant {
    if (emphasis === 'accent') return 'info';
    if (emphasis === 'muted') return 'muted';
    return enabled > 0 ? 'success' : 'neutral';
  }

  function placementBadge(state: WorldPackState | null): {
    variant: BadgeVariant;
    label: string;
  } {
    switch (state) {
      case 'enabled':
        return { variant: 'success', label: get(t)('worlds.datapacks.stateEnabled') };
      case 'disabled':
        return { variant: 'muted', label: get(t)('worlds.datapacks.stateDisabled') };
      case 'orphaned':
        return { variant: 'danger', label: get(t)('worlds.datapacks.stateOrphaned') };
      case 'not_added':
        return { variant: 'neutral', label: get(t)('worlds.datapacks.stateNotAdded') };
      default:
        return { variant: 'neutral', label: get(t)('addons.datapacks.stateUnknown') };
    }
  }
</script>

<div class="p-3" data-testid="installed-datapacks">
  <div class="flex items-center justify-end gap-2 mb-2">
    <!-- `mr-auto` lives here and NOWHERE else in this row: it is what pins the
         left slot, and the gate note below (which used to own it) now simply
         follows the popover. Two `mr-auto` siblings would fight over the free
         space and push the note off the left edge. -->
    <span class="mr-auto inline-flex items-center">
      <HelpPopover
        paragraphs={datapackConceptParas}
        width={320}
        triggerAriaLabel={$t('onboarding.datapackConcept.triggerAriaLabel')}
        triggerTitle={$t('onboarding.datapackConcept.triggerTitle')}
        closeAriaLabel={$t('onboarding.datapackConcept.closeAriaLabel')}
      />
    </span>
    {#if disabledKey !== null && !busy}
      <!-- Why every mutation below is disabled: the game owns level.dat while
           it runs, and the backend refuses datapack writes for the duration.
           One visible line beats six tooltips on unfocusable disabled
           buttons. -->
      <p class="text-xs text-warning-text" data-testid="datapacks-gate-note">
        {$t(disabledKey)}
      </p>
    {/if}
    {#if updateCount > 0}
      <BusyButton
        type="button"
        class="btn-warning btn-sm"
        {busy}
        disabled={checking || instanceId === null || disabledKey !== null}
        onclick={updateAll}
      >
        {$t('addons.installed.updateAll', { count: updateCount })}
      </BusyButton>
    {/if}
    <BusyButton
      type="button"
      class="btn-secondary btn-sm"
      busy={checking}
      disabled={busy || instanceId === null || (view?.entries.length ?? 0) === 0}
      onclick={checkUpdates}
    >
      <Icon name="refresh" class="icon-spin-hover" />
      {$t('addons.installed.checkUpdates')}
    </BusyButton>
    <button
      type="button"
      class="btn-secondary btn-sm"
      disabled={busy || instanceId === null || mcVersion === null}
      onclick={() => (vtOpen = true)}
      data-testid="open-vt-builder"
    >
      {$t('addons.datapacks.vt.open')}
    </button>
  </div>

  {#if error}
    <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2 mb-2">
      {error}
    </div>
  {/if}

  {#if instanceId === null}
    <div class="text-muted text-sm py-6 text-center">{$t('addons.installed.pickInstance')}</div>
  {:else if loading && view === null}
    <LoadingPanel label={$t('addons.installed.loading')} size="md" />
  {:else if (view?.entries.length ?? 0) === 0}
    <div class="text-muted text-sm py-6 text-center">{$t('addons.datapacks.empty')}</div>
  {:else if view}
    <div class="border border-border-subtle rounded-lg overflow-hidden">
      {#each view.entries as entry (entry.pack.filename)}
        {@const latest = updatable(entry.pack.filename)}
        {@const summary = datapackWorldSummary(entry.placements)}
        {@const failReason = checkFailedReason(entry.pack.filename)}
        {@const isOpen = expanded.has(entry.pack.filename)}
        <div class="border-b border-border-subtle last:border-b-0">
          <CardShell
            variant="compact-row"
            accent={entry.compat.kind === 'mismatch' ? 'warning' : latest ? 'warning' : 'none'}
          >
            <button
              type="button"
              class="btn-icon btn-icon-sm"
              aria-expanded={isOpen}
              aria-label={$t('addons.datapacks.expandAria', { name: entry.pack.name })}
              data-testid="datapack-row-expand"
              onclick={() => {
                if (isOpen) expanded.delete(entry.pack.filename);
                else expanded.add(entry.pack.filename);
              }}
            >
              <Icon name={isOpen ? 'chevronDown' : 'chevronRight'} size={15} />
            </button>
            {#if entry.pack.source && entry.pack.project_id}
              <button
                type="button"
                class="flex flex-1 items-center gap-2.5 min-w-0 text-left"
                onclick={() => openDetail(entry)}
              >
                <CardMedia
                  iconUrl={summaries.get(entry.pack.filename)?.icon_url ?? null}
                  placeholder="datapack"
                  size="sm"
                />
                <span class="min-w-0 flex-1 truncate">
                  <span class="text-sm text-primary">{entry.pack.name}</span>
                  {#if entry.pack.version_number}
                    <span class="text-xs text-muted ml-2">{entry.pack.version_number}</span>
                  {/if}
                </span>
              </button>
            {:else}
              <CardMedia iconUrl={null} placeholder="datapack" size="sm" />
              <div class="min-w-0 flex-1 truncate">
                <span class="text-sm text-primary">{entry.pack.name}</span>
                <span
                  class="text-xs text-muted ml-2 font-mono"
                  use:tooltip={{ text: entry.pack.filename, whenOverflowing: true }}
                >
                  {entry.pack.filename}
                </span>
              </div>
            {/if}
            <StatusBadge
              variant={summaryVariant(summary.emphasis, summary.args.enabled)}
              testid="datapack-world-summary"
            >
              {$t(summary.key, summary.args)}
            </StatusBadge>
            {#if !entry.in_library}
              <StatusBadge variant="neutral" icon="warning" testid="datapack-only-in-worlds">
                {$t('addons.datapacks.onlyInWorlds')}
              </StatusBadge>
            {/if}
            {#if entry.compat.kind === 'mismatch'}
              <!-- Once, on the collapsed row: the verdict is per-instance. -->
              <span
                class="text-warning-text text-xs flex-shrink-0"
                use:tooltip={$t('worlds.datapacks.formatMismatch', {
                  packFormat: entry.compat.pack_format,
                  expected: entry.compat.expected,
                })}
                aria-label={$t('worlds.datapacks.formatMismatch', {
                  packFormat: entry.compat.pack_format,
                  expected: entry.compat.expected,
                })}
                role="img"
              >
                <Icon name="warning" size={15} />
              </span>
            {/if}
            {#if failReason}
              <span
                class="text-warning-text flex-shrink-0"
                use:tooltip={failReason}
                aria-label={$t('addons.installed.checkFailed')}
                role="img"
              >
                <Icon name="warning" size={15} />
              </span>
            {/if}
            {#if latest && entry.pack.source && changelogSupported(entry.pack.source)}
              <button
                type="button"
                class="btn-icon btn-icon-sm"
                disabled={busy}
                onclick={() => openAssetChangelog(entry, latest)}
                aria-label={$t('mods.changelog.view')}
                use:tooltip={$t('mods.changelog.view')}><Icon name="scrollText" size={15} /></button
              >
            {/if}
            {#if latest}
              <button
                type="button"
                class="btn-icon btn-icon-sm btn-icon-warning"
                disabled={gated}
                onclick={() => update(entry, latest)}
                aria-label={$t('addons.installed.update')}
                use:tooltip={$t('addons.installed.update')}
                data-testid="datapack-update-btn"><Icon name="refresh" size={15} /></button
              >
            {/if}
            {#if entry.in_library}
              <button
                type="button"
                class="btn-icon btn-icon-sm !text-accent"
                disabled={gated}
                onclick={() => (pickerFor = entry)}
                aria-label={$t('addons.datapacks.addToWorlds')}
                use:tooltip={$t('addons.datapacks.addToWorlds')}
                data-testid="datapack-add-to-worlds"><Icon name="plus" size={15} /></button
              >
            {/if}
            <button
              type="button"
              class="btn-icon btn-icon-sm btn-icon-danger"
              disabled={gated}
              onclick={() => (removeFor = entry)}
              aria-label={$t('addons.installed.remove')}
              use:tooltip={$t('addons.installed.remove')}
              data-testid="datapack-remove-btn"><Icon name="trash" size={15} /></button
            >
          </CardShell>
          {#if isOpen}
            <div class="bg-surface px-3 py-1.5" data-testid="datapack-placements">
              {#if entry.placements.length === 0}
                <p class="text-xs text-muted py-1">{$t('addons.datapacks.noPlacements')}</p>
              {:else}
                {#each entry.placements as p (p.world)}
                  {@const badge = placementBadge(p.state)}
                  <div class="flex items-center gap-2 py-1 text-sm">
                    <Icon name="datapack" size={14} class="text-muted flex-shrink-0" />
                    <span class="flex-1 min-w-0 truncate text-primary">{p.world}</span>
                    <StatusBadge variant={badge.variant}>{badge.label}</StatusBadge>
                    {#if p.state === 'enabled' || p.state === 'disabled'}
                      <button
                        type="button"
                        class={`btn-icon btn-icon-sm ${p.state === 'enabled' ? 'btn-icon-success' : '!text-muted'}`}
                        disabled={gated}
                        aria-label={p.state === 'enabled'
                          ? $t('worlds.datapacks.disable')
                          : $t('worlds.datapacks.enable')}
                        use:tooltip={p.state === 'enabled'
                          ? $t('worlds.datapacks.disable')
                          : $t('worlds.datapacks.enable')}
                        onclick={() => toggleInWorld(entry, p.world, p.state as WorldPackState)}
                      >
                        {#if busy}<Spinner size="sm" />{:else}<Icon name="power" size={15} />{/if}
                      </button>
                    {/if}
                    {#if p.state !== null}
                      <button
                        type="button"
                        class="btn-icon btn-icon-sm btn-icon-danger"
                        disabled={gated}
                        aria-label={$t('worlds.datapacks.removeFromWorld')}
                        use:tooltip={$t('worlds.datapacks.removeFromWorld')}
                        onclick={() => removeFromWorld(entry, p.world)}
                      >
                        <Icon name="trash" size={15} />
                      </button>
                    {/if}
                  </div>
                {/each}
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  {#if detail && instanceId}
    <ModDetailModal
      source={detail.source}
      projectId={detail.projectId}
      {mcVersion}
      {loader}
      kind="datapack"
      installedVersionId={detail.versionId}
      onClose={() => {
        detail = null;
        detailFor = null;
      }}
      onInstall={installDetailVersion}
    />
  {/if}

  {#if pickerFor && instanceId}
    <DatapackWorldPicker
      {instanceId}
      filename={pickerFor.pack.filename}
      packName={pickerFor.pack.name}
      placements={pickerFor.placements}
      onClose={() => (pickerFor = null)}
      onApplied={() => {
        void refresh();
        datapacksChanged.value++;
      }}
    />
  {/if}

  {#if removeFor && instanceId}
    <DatapackRemoveDialog
      {instanceId}
      filename={removeFor.pack.filename}
      packName={removeFor.pack.name}
      placements={removeFor.placements}
      inLibrary={removeFor.in_library}
      onClose={() => (removeFor = null)}
      onRemoved={() => {
        void refresh();
        datapacksChanged.value++;
      }}
    />
  {/if}

  {#if vtOpen && mcVersion}
    <Modal
      onClose={() => (vtOpen = false)}
      ariaLabel={$t('addons.datapacks.vt.title')}
      panelClass="max-w-2xl w-full"
      dataTestid="vt-builder-modal"
    >
      <VanillaTweaksBuilder {mcVersion} installed={vtInstalled} busy={vtBusy} onBuild={buildVt} />
    </Modal>
  {/if}

  {#if changelogReq}
    <ChangelogModal
      source={changelogReq.source}
      projectId={changelogReq.projectId}
      title={changelogReq.title}
      targetVersionId={changelogReq.target}
      baseVersionId={changelogReq.base}
      onClose={() => (changelogReq = null)}
    />
  {/if}
</div>
