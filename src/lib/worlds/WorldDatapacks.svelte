<script lang="ts">
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import {
    commands,
    type LevelDatPresence,
    type WorldDatapack,
    type WorldPackState,
  } from '$lib/ipc/bindings';
  import { ignoredHintKey, ignoredLabelKey } from '$lib/worlds/datapack-state';
  import { compatLine, type CompatLine } from '$lib/worlds/datapack-compat';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import DatapackConceptHelp from '$lib/onboarding/DatapackConceptHelp.svelte';
  import { pushInfo } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import CardShell from '$lib/ui/cards/CardShell.svelte';
  import CardMedia from '$lib/ui/cards/CardMedia.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import type { CardAccent, BadgeVariant } from '$lib/ui/cards/card-status';
  import { worldDatapacksDisabledKey, worldRowKind } from '$lib/worlds/datapacks-gating';
  import DatapackRemoveDialog from '$lib/mods/DatapackRemoveDialog.svelte';
  import { warnFailedRefresh } from '$lib/mods/datapack-refresh-warning';
  import { onDestroy } from 'svelte';
  import BulkActionBar, { type BulkBarAction } from '$lib/ui/BulkActionBar.svelte';
  import SelectRowCheckbox from '$lib/ui/SelectRowCheckbox.svelte';
  import { reportBulk, runBulk } from '$lib/ui/bulk-run';
  import { createListSelection } from '$lib/ui/list-selection.svelte';
  import { refocusAfterRemoval } from '$lib/ui/refocus-after-removal';
  import DatapackBulkRemoveDialog, {
    type ThisWorldEntry,
  } from '$lib/mods/DatapackBulkRemoveDialog.svelte';
  import { type WorldBulkAction, worldBulkApplies } from '$lib/worlds/datapack-bulk';

  // Per-world datapack manager. Library ∪ on-disk ∪ level.dat names, each row
  // carrying its own state. A "ghost" (a level.dat name whose file is gone) is
  // not a problem: Minecraft logs "Missing data pack", skips it and drops the
  // name at its next save. So it is rendered quietly and its one action,
  // "Clear entry", only tidies the list sooner (see worldRowKind). An ignored
  // row is an entry the game does not load: it gets its reason and removal
  // only (spec §2 N.6).
  let {
    instanceId,
    world,
    running = false,
  }: { instanceId: string; world: string; running?: boolean } = $props();

  let packs = $state<WorldDatapack[]>([]);
  // The world's level.dat presence (D2): `null` until loaded, or when the load
  // failed.
  let levelDat = $state<LevelDatPresence | null>(null);
  let loadError = $state<string | null>(null);
  let actionError = $state<string | null>(null);
  let busy = $state(false);
  let busyAdd = $state(false);
  let busyRow = $state<string | null>(null);
  // The entry the this-world removal confirmation is open for (U1): every
  // trash that can delete a file asks first. Only the ghost's "remove"
  // (`world-datapack-remove-orphaned`) deletes nothing and stays one click.
  let removeTarget = $state<string | null>(null);

  // running > level.dat > busy (worldDatapacksDisabledKey). Every mutating
  // control below reads `disabledKey`, so D2 covers both Add buttons, `+`, the
  // toggles, Clear entry and the trash. `null` — not loaded yet, or the load
  // failed — is not a verdict there: the per-row controls only exist once a
  // listing arrived (a failed load clears the rows), and their world writers
  // re-check level.dat first (§3 L.4). The two Add buttons are different: they
  // install into the library BEFORE the world writer refuses anything, so
  // they also wait until the world's level.dat is known (`addBlocked`).
  const disabledKey = $derived(worldDatapacksDisabledKey({ running, busy, levelDat }));
  const addBlocked = $derived(disabledKey !== null || levelDat === null);
  const disabledReason = $derived.by(() => {
    const key = disabledKey;
    return key === null ? null : $t(key);
  });
  // Add's own reason. Its wait for the listing is a gate `disabledKey` does
  // not carry, and a disabled control must still say why and keep its tab stop
  // (DESIGN.md §5), whether the listing is on its way or failed to arrive.
  const addBlockedReason = $derived(
    disabledReason ?? (addBlocked ? $t('worlds.datapacks.blockedUntilListed') : null),
  );
  // Nothing has been read for this world yet: not an empty list.
  const awaitingListing = $derived(levelDat === null && loadError === null);

  async function reload() {
    // Capture the world this load is for; a rapid world switch mid-fetch must
    // not commit the previous world's rows over the newer selection.
    const reqInstanceId = instanceId;
    const reqWorld = world;
    loadError = null;
    const res = await commands.datapacksListForWorld(reqInstanceId, reqWorld);
    if (instanceId !== reqInstanceId || world !== reqWorld) return;
    if (res.status === 'ok') {
      packs = res.data.packs;
      levelDat = res.data.level_dat;
    } else {
      // Clear the list on failure too: an error and a stale, still-interactive
      // row list must never render together (see the template's mutually
      // exclusive loadError / empty / list branches below) — a row surviving
      // a failed reload would contradict whatever action just triggered it.
      packs = [];
      levelDat = null;
      loadError = formatError(res.error);
    }
  }

  $effect(() => {
    void instanceId;
    void world;
    // A stale action error from the PREVIOUS world must not survive a world
    // switch and render on top of the new world's (perfectly valid) pack
    // list — same "stale message outlives its context" defect family as the
    // loadError fix above, just triggered by a prop change instead of a
    // failed reload.
    actionError = null;
    removeTarget = null;
    bulkRemoveFor = null;
    // The previous world's level.dat note must not stand over this one, and
    // neither must its rows: until this world's listing arrives they would
    // act on this world under the previous world's names.
    levelDat = null;
    packs = [];
    void reload();
  });

  function stateLabel(pack: WorldDatapack): string {
    switch (pack.state) {
      case 'enabled':
        return $t('worlds.datapacks.stateEnabled');
      case 'disabled':
        return $t('worlds.datapacks.stateDisabled');
      case 'not_added':
        return $t('worlds.datapacks.stateNotAdded');
      case 'orphaned':
        return $t('worlds.datapacks.stateOrphaned');
      case 'ignored':
        return $t(ignoredLabelKey(pack.ignored_reason));
    }
  }

  function stateBadgeVariant(state: WorldPackState): BadgeVariant {
    switch (state) {
      case 'enabled':
        return 'success';
      case 'disabled':
        return 'muted';
      case 'not_added':
        return 'neutral';
      case 'orphaned':
        return 'neutral';
      case 'ignored':
        return 'warning';
    }
  }

  // The verdict's own sentence for this row, or null. An ignored row speaks
  // through its reason hint (a not-loadable one names the same cause), and a
  // ghost whose file is gone has nothing left to judge.
  function rowCompatLine(pack: WorldDatapack): CompatLine | null {
    const kind = worldRowKind(pack);
    if (kind === 'ignored' || kind === 'ghost') return null;
    return compatLine(pack.compat);
  }

  // A ghost is quiet (the game heals it itself); otherwise a compatibility
  // problem outranks "it's off", which follows the shared muted convention
  // from card-status.ts (see rowDim below).
  function rowAccent(pack: WorldDatapack): CardAccent {
    if (worldRowKind(pack) === 'ghost') return 'none';
    if (pack.state === 'ignored') return 'warning';
    if (rowCompatLine(pack) !== null) return 'warning';
    if (pack.state === 'disabled') return 'muted';
    return 'none';
  }

  // Mirrors card-status.ts's `disabled: { dim: true }` — every other CardShell
  // surface dims a disabled row, so a disabled datapack shouldn't read as
  // equally prominent as an enabled one.
  function rowDim(pack: WorldDatapack): boolean {
    return pack.state === 'disabled';
  }

  // `source` picks the Tauri dialog mode: a `.zip` file, or a folder datapack
  // (the backend zips a directory in memory — see
  // `datapacks::library::install_local_at` — so both are equally supported,
  // but `open()` cannot offer a file-or-folder chooser in one dialog).
  async function addDatapackToLibrary(source: 'zip' | 'folder') {
    actionError = null;
    const picked =
      source === 'zip'
        ? await openFile({
            multiple: false,
            filters: [{ name: $t('common.fileFilter.datapack'), extensions: ['zip'] }],
          })
        : await openFile({ directory: true });
    if (typeof picked !== 'string') return;
    busyAdd = true;
    busy = true;
    try {
      const installed = await commands.datapacksInstallFromFile(instanceId, picked);
      if (installed.status !== 'ok') {
        actionError = formatError(installed.error);
        return;
      }
      // A same-named pack was replaced: a world left on its old bytes is named.
      warnFailedRefresh(installed.data.pack.name, installed.data.refreshed);
      const placed = await commands.datapacksAddToWorld(
        instanceId,
        world,
        installed.data.pack.filename,
      );
      if (placed.status === 'ok') {
        if (placed.data === 'copied') pushInfo($t('worlds.datapacks.copyNotLinked'));
      } else {
        // The install itself succeeded — the pack IS now in the library, and
        // must show up as `not_added` rather than silently vanishing — so
        // reload runs below regardless of whether adding it to THIS world
        // also succeeded.
        actionError = formatError(placed.error);
      }
      await reload();
    } finally {
      busyAdd = false;
      busy = false;
    }
  }

  async function addToWorld(filename: string) {
    busyRow = filename;
    busy = true;
    actionError = null;
    try {
      const res = await commands.datapacksAddToWorld(instanceId, world, filename);
      if (res.status === 'ok') {
        if (res.data === 'copied') pushInfo($t('worlds.datapacks.copyNotLinked'));
        await reload();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      busyRow = null;
      busy = false;
    }
  }

  async function removeFromWorld(filename: string) {
    busyRow = filename;
    busy = true;
    actionError = null;
    try {
      const res = await commands.datapacksRemoveFromWorld(instanceId, world, filename);
      if (res.status === 'ok') {
        await reload();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      busyRow = null;
      busy = false;
    }
  }

  async function toggleEnabled(pack: WorldDatapack) {
    busyRow = pack.filename;
    busy = true;
    actionError = null;
    try {
      const res = await commands.datapacksSetEnabledInWorld(
        instanceId,
        world,
        pack.filename,
        pack.state !== 'enabled',
      );
      if (res.status === 'ok') {
        await reload();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      busyRow = null;
      busy = false;
    }
  }

  // The bulk bar (DESIGN.md §8): keys are filenames; the selection is cleared when the world or
  // the profile changes.
  const selection = createListSelection(
    () => packs.map((p) => p.filename),
    () => `${instanceId}\u0000${world}`,
  );
  onDestroy(() => selection.dispose());
  let bulkAction = $state<WorldBulkAction | null>(null);
  let bulkRemoveFor = $state<ThisWorldEntry[] | null>(null);
  let bulkRemoveIndex = 0;
  let listEl = $state<HTMLElement | null>(null);
  let emptyListEl = $state<HTMLElement | null>(null);

  const selectedPacks = $derived(packs.filter((p) => selection.selected.has(p.filename)));
  const applicable = (action: WorldBulkAction) =>
    selectedPacks.filter((p) => worldBulkApplies(p, action));
  // The world's own gate (running, level.dat, busy) first, with its text; otherwise an action is
  // off when no selected row can take it.
  function bulkGate(action: BulkBarAction, none: boolean): BulkBarAction {
    if (disabledKey !== null) return { ...action, disabled: true, disabledReason: $t(disabledKey) };
    return { ...action, disabled: none, disabledReason: $t('ui.bulk.noneApplicable') };
  }
  const bulkActions = $derived<BulkBarAction[]>([
    bulkGate(
      { id: 'enable', label: $t('worlds.datapacks.enable') },
      applicable('enable').length === 0,
    ),
    bulkGate(
      { id: 'disable', label: $t('worlds.datapacks.disable') },
      applicable('disable').length === 0,
    ),
    bulkGate(
      { id: 'add', label: $t('worlds.datapacks.addToWorld') },
      applicable('add').length === 0,
    ),
    bulkGate(
      { id: 'remove', label: $t('worlds.datapacks.removeFromWorld'), intent: 'danger' },
      applicable('remove').length === 0,
    ),
  ]);
  function onBulkAction(id: string): void {
    if (id === 'enable') void bulkSetEnabled(true);
    else if (id === 'disable') void bulkSetEnabled(false);
    else if (id === 'add') void bulkAdd();
    else if (id === 'remove') requestBulkRemove();
  }

  async function bulkSetEnabled(enable: boolean): Promise<void> {
    const reqInstance = instanceId;
    const reqWorld = world;
    const targets = applicable(enable ? 'enable' : 'disable');
    if (disabledKey !== null || targets.length === 0) return;
    bulkAction = enable ? 'enable' : 'disable';
    busy = true;
    actionError = null;
    try {
      const outcome = await runBulk(
        targets,
        (p) => commands.datapacksSetEnabledInWorld(reqInstance, reqWorld, p.filename, enable),
        formatError,
        (p) => p.filename,
      );
      reportBulk(
        outcome,
        enable
          ? { done: 'worlds.datapacks.bulkEnabled', partial: 'worlds.datapacks.bulkEnabledFailed' }
          : {
              done: 'worlds.datapacks.bulkDisabled',
              partial: 'worlds.datapacks.bulkDisabledFailed',
            },
      );
      await reload();
    } finally {
      busy = false;
      bulkAction = null;
    }
    selection.clear();
  }

  // One «copied, not linked» note for the run, however many answered `copied`.
  async function bulkAdd(): Promise<void> {
    const reqInstance = instanceId;
    const reqWorld = world;
    const targets = applicable('add');
    if (disabledKey !== null || targets.length === 0) return;
    bulkAction = 'add';
    busy = true;
    actionError = null;
    let copied = false;
    try {
      const outcome = await runBulk(
        targets,
        async (p) => {
          const res = await commands.datapacksAddToWorld(reqInstance, reqWorld, p.filename);
          if (res.status === 'ok' && res.data === 'copied') copied = true;
          return res;
        },
        formatError,
        (p) => p.filename,
      );
      if (copied) pushInfo($t('worlds.datapacks.copyNotLinked'));
      reportBulk(outcome, {
        done: 'worlds.datapacks.bulkAdded',
        partial: 'worlds.datapacks.bulkAddedFailed',
      });
      await reload();
    } finally {
      busy = false;
      bulkAction = null;
    }
    selection.clear();
  }

  function requestBulkRemove(): void {
    const targets = applicable('remove');
    if (disabledKey !== null || targets.length === 0) return;
    actionError = null;
    bulkRemoveIndex = Math.max(
      0,
      packs.findIndex((p) => selection.selected.has(p.filename)),
    );
    bulkRemoveFor = targets.map((p) => ({
      filename: p.filename,
      name: p.filename,
      ghost: worldRowKind(p) === 'ghost',
    }));
  }

  async function afterBulkRemove(): Promise<void> {
    selection.clear();
    await reload();
    await refocusAfterRemoval({
      index: bulkRemoveIndex,
      listEl,
      rows: (list) => [...list.querySelectorAll<HTMLElement>('[data-testid="world-datapack-row"]')],
      emptyEl: emptyListEl,
    });
  }
</script>

<div class="flex flex-col gap-2" data-testid="world-datapacks">
  <div class="flex items-center justify-between gap-2">
    <div class="flex items-center gap-1">
      <h4 class="text-sm font-medium text-primary">{$t('worlds.datapacks.title')}</h4>
      <DatapackConceptHelp />
    </div>
    <div class="flex items-center gap-2">
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <span
        class="inline-flex"
        tabindex={addBlocked ? 0 : undefined}
        use:tooltip={{ text: addBlockedReason ?? '', describe: false }}
      >
        <BusyButton
          class="btn-secondary btn-sm"
          busy={busyAdd}
          disabled={addBlocked}
          onclick={() => void addDatapackToLibrary('zip')}
          data-testid="world-datapack-add-library"
        >
          <Icon name="archive" size={14} />
          {$t('worlds.datapacks.add')}
        </BusyButton>
      </span>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <span
        class="inline-flex"
        tabindex={addBlocked ? 0 : undefined}
        use:tooltip={{ text: addBlockedReason ?? '', describe: false }}
      >
        <button
          type="button"
          class="btn-tertiary inline-flex items-center gap-1"
          disabled={addBlocked}
          data-testid="world-datapack-add-library-folder"
          onclick={() => void addDatapackToLibrary('folder')}
        >
          <Icon name="folderOpen" size={14} />
          {$t('worlds.datapacks.addFromFolder')}
        </button>
      </span>
    </div>
  </div>

  {#if levelDat === 'absent' || levelDat === 'only_old'}
    <p class="text-xs text-warning-text" data-testid="world-datapacks-level-dat-note">
      {$t(
        levelDat === 'absent' ? 'worlds.datapacks.noLevelDat' : 'worlds.datapacks.onlyOldLevelDat',
      )}
    </p>
  {/if}

  {#if actionError}
    <p class="text-sm text-danger">{actionError}</p>
  {/if}

  {#if loadError}
    <p class="text-sm text-danger">{loadError}</p>
  {:else if awaitingListing}
    <div class="flex justify-center py-3 text-secondary" data-testid="world-datapacks-loading">
      <Spinner labelPlacement="below" label={$t('common.loading')} />
    </div>
  {:else if packs.length === 0}
    <!-- For a folder with no level.dat the note above replaces "No datapacks
         yet" (§3 L.8): the game loads nothing from it at all. -->
    {#if levelDat !== 'absent'}
      <!-- Focus lands here after the last removal (`refocusAfterRemoval`): a parking place
           that reads the message, not a control. -->
      <p class="text-sm text-muted outline-none" tabindex="-1" bind:this={emptyListEl}>
        {$t('worlds.datapacks.empty')}
      </p>
    {/if}
  {:else}
    <div class="overflow-hidden rounded-lg border border-border-subtle" bind:this={listEl}>
      <BulkActionBar
        allSelected={selection.allSelected}
        indeterminate={selection.indeterminate}
        selectedCount={selection.count}
        busy={disabledKey !== null || bulkAction !== null}
        busyAction={bulkAction}
        hint={$t('worlds.datapacks.bulkHint')}
        actions={bulkActions}
        onToggleAll={selection.toggleAll}
        onAction={onBulkAction}
        onClear={selection.clear}
      />
      {#each packs as pack (pack.filename)}
        {@const kind = worldRowKind(pack)}
        {@const compatWarn = rowCompatLine(pack)}
        <CardShell
          variant="compact-row"
          accent={rowAccent(pack)}
          dim={rowDim(pack)}
          testid="world-datapack-row"
        >
          <SelectRowCheckbox
            checked={selection.selected.has(pack.filename)}
            name={pack.filename}
            onChange={(c) => selection.toggle(pack.filename, c)}
          />
          <CardMedia placeholder="package" size="sm" />
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span
                class="font-mono text-xs truncate text-primary"
                use:tooltip={{ text: pack.filename, whenOverflowing: true }}
              >
                {pack.filename}
              </span>
              <StatusBadge variant={kind === 'ghost' ? 'neutral' : stateBadgeVariant(pack.state)}>
                {kind === 'ghost' ? $t('worlds.datapacks.stateOrphaned') : stateLabel(pack)}
              </StatusBadge>
              <!-- A ghost has no file to judge, so no compatibility badge either. -->
              {#if kind !== 'ignored' && kind !== 'ghost' && pack.compat.kind === 'unknown'}
                <StatusBadge variant="neutral" icon="info"
                  >{$t('worlds.datapacks.compatUnknown')}</StatusBadge
                >
              {/if}
              {#if !pack.in_library}
                <StatusBadge variant="neutral">{$t('worlds.datapacks.external')}</StatusBadge>
              {/if}
            </div>
            {#if compatWarn}
              <p class="mt-0.5 text-xs text-warning-text" data-testid="world-datapack-compat">
                {$t(compatWarn.key, compatWarn.args)}
              </p>
            {/if}
            {#if kind === 'ghost'}
              <p class="mt-0.5 text-xs text-muted">{$t('worlds.datapacks.orphanedHint')}</p>
            {/if}
            {#if pack.state === 'ignored'}
              {@const hint = ignoredHintKey(pack.ignored_reason, pack.compat)}
              {#if hint}<p
                  class="mt-0.5 text-xs text-warning-text"
                  data-testid="world-datapack-ignored-hint"
                >
                  {$t(hint)}
                </p>{/if}
            {/if}
          </div>
          <div class="flex flex-shrink-0 items-center gap-1">
            {#if kind === 'ignored'}
              <!-- The game does not load this entry, so there is nothing to
                   switch on or off: removal only (spec §2 N.6). -->
              <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
              <span
                class="inline-flex"
                tabindex={disabledKey !== null || busyRow === pack.filename ? 0 : undefined}
                use:tooltip={{
                  text: disabledReason ?? $t('worlds.datapacks.removeFromWorld'),
                  describe: false,
                }}
              >
                <button
                  type="button"
                  class="btn-icon btn-icon-sm btn-icon-danger"
                  data-testid="world-datapack-remove-world"
                  disabled={disabledKey !== null || busyRow === pack.filename}
                  aria-label={$t('worlds.datapacks.removeFromWorld')}
                  onclick={() => (removeTarget = pack.filename)}
                >
                  {#if busyRow === pack.filename}
                    <Spinner size="sm" />
                  {:else}
                    <Icon name="trash" size={15} />
                  {/if}
                </button>
              </span>
            {:else if kind === 'ghost'}
              <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
              <span
                class="inline-flex"
                tabindex={disabledKey !== null ? 0 : undefined}
                use:tooltip={{ text: disabledReason ?? '', describe: false }}
              >
                <!-- Clears the level.dat name only: there is no file to delete,
                     so it needs no confirmation (U3). -->
                <BusyButton
                  class="btn-secondary btn-sm"
                  busy={busyRow === pack.filename}
                  disabled={disabledKey !== null}
                  onclick={() => void removeFromWorld(pack.filename)}
                  data-testid="world-datapack-remove-orphaned"
                >
                  {$t('worlds.datapacks.clearEntry')}
                </BusyButton>
              </span>
            {:else if kind === 'addable'}
              <!-- §0.5 I11: this version skips the pack; the engine would accept the file and load nothing, so no add is offered. -->
              {#if pack.compat.kind !== 'wont_load'}
                <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
                <span
                  class="inline-flex"
                  tabindex={disabledKey !== null || busyRow === pack.filename ? 0 : undefined}
                  use:tooltip={{
                    text: disabledReason ?? $t('worlds.datapacks.addToWorld'),
                    describe: false,
                  }}
                >
                  <button
                    type="button"
                    class="btn-icon btn-icon-sm !text-accent"
                    data-testid="world-datapack-add-world"
                    disabled={disabledKey !== null || busyRow === pack.filename}
                    aria-label={$t('worlds.datapacks.addToWorld')}
                    onclick={() => void addToWorld(pack.filename)}
                  >
                    {#if busyRow === pack.filename}
                      <Spinner size="sm" />
                    {:else}
                      <Icon name="plus" size={15} />
                    {/if}
                  </button>
                </span>
              {/if}
            {:else}
              <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
              <span
                class="inline-flex"
                tabindex={disabledKey !== null || busyRow === pack.filename ? 0 : undefined}
                use:tooltip={{
                  text:
                    disabledReason ??
                    (pack.state === 'enabled'
                      ? $t('worlds.datapacks.disable')
                      : $t('worlds.datapacks.enable')),
                  describe: false,
                }}
              >
                <button
                  type="button"
                  class={`btn-icon btn-icon-sm ${pack.state === 'enabled' ? 'btn-icon-success' : '!text-muted'}`}
                  data-testid="world-datapack-toggle"
                  disabled={disabledKey !== null || busyRow === pack.filename}
                  aria-label={pack.state === 'enabled'
                    ? $t('worlds.datapacks.disable')
                    : $t('worlds.datapacks.enable')}
                  onclick={() => void toggleEnabled(pack)}
                >
                  {#if busyRow === pack.filename}
                    <Spinner size="sm" />
                  {:else}
                    <Icon name="power" size={15} />
                  {/if}
                </button>
              </span>
              <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
              <span
                class="inline-flex"
                tabindex={disabledKey !== null || busyRow === pack.filename ? 0 : undefined}
                use:tooltip={{
                  text: disabledReason ?? $t('worlds.datapacks.removeFromWorld'),
                  describe: false,
                }}
              >
                <button
                  type="button"
                  class="btn-icon btn-icon-sm btn-icon-danger"
                  data-testid="world-datapack-remove-world"
                  disabled={disabledKey !== null || busyRow === pack.filename}
                  aria-label={$t('worlds.datapacks.removeFromWorld')}
                  onclick={() => (removeTarget = pack.filename)}
                >
                  {#if busyRow === pack.filename}
                    <Spinner size="sm" />
                  {:else}
                    <Icon name="trash" size={15} />
                  {/if}
                </button>
              </span>
            {/if}
          </div>
        </CardShell>
      {/each}
    </div>
  {/if}

  {#if removeTarget !== null}
    <DatapackRemoveDialog
      {instanceId}
      filename={removeTarget}
      packName={removeTarget}
      mode={{ kind: 'this-world', world }}
      onClose={() => (removeTarget = null)}
      onRemoved={() => void reload()}
    />
  {/if}

  {#if bulkRemoveFor !== null}
    <DatapackBulkRemoveDialog
      {instanceId}
      mode={{ kind: 'this-world', world, entries: bulkRemoveFor }}
      onRunning={(running) => {
        busy = running;
        bulkAction = running ? 'remove' : null;
      }}
      onClose={() => (bulkRemoveFor = null)}
      onRemoved={() => void afterBulkRemove()}
    />
  {/if}
</div>
