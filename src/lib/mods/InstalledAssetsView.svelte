<script lang="ts">
  // Installed-assets pane for resource packs / shaders. A deliberately
  // simpler sibling of the mods Installed view: no enable/disable, no
  // dependency/orphan graph — just list, Remove, and Check-updates +
  // per-row Update. AddonsTab (Task 11) chooses between this and
  // InstalledModsView by `kind`.
  import {
    commands,
    type AssetUpdateState,
    type ContentKind,
    type InstalledAsset,
    type LoaderKind,
    type ModSource,
    type ModSummary,
    type ModVersion,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { assetsChanged } from '$lib/settings/state.svelte';
  import { t } from '$lib/i18n';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import { Icon } from '$lib/ui/icons';
  import CardShell from '$lib/ui/cards/CardShell.svelte';
  import CardMedia from '$lib/ui/cards/CardMedia.svelte';
  import ModDetailModal from './ModDetailModal.svelte';
  import { mapLimit } from './concurrency';
  import { get } from 'svelte/store';
  import { tooltip } from '$lib/ui/tooltip';

  let {
    instanceId,
    kind,
    mcVersion = null,
    loader = null,
  }: {
    instanceId: string | null;
    kind: ContentKind;
    mcVersion?: string | null;
    loader?: LoaderKind | null;
  } = $props();

  // Project summaries (icon_url) for assets that have a platform identity,
  // resolved like installed mods via commands.modsProject. Keyed by filename.
  let summaries = $state<Map<string, ModSummary>>(new Map());
  // Detail modal target — a resource pack / shader IS a Modrinth/CF project, so
  // the same ModDetailModal works; install routes through assetInstall.
  let detail = $state<{ source: ModSource; projectId: string; versionId: string | null } | null>(
    null,
  );

  let assets = $state<InstalledAsset[]>([]);
  let loading = $state(false);
  let busy = $state(false);
  let checking = $state(false);
  let error = $state<string | null>(null);
  // Update-check results keyed by filename — only entries that resolved to a
  // concrete state are kept (up_to_date / update_available / check_failed).
  let updateStates = $state<Map<string, AssetUpdateState>>(new Map());

  // Refetch whenever the instance OR kind changes. A bare $effect re-runs on
  // any read dependency it touches; reading both here guarantees a switch
  // between resource_pack and shader (or instances) re-lists. A generation
  // counter discards a stale in-flight response if the inputs change mid-fetch.
  let generation = 0;
  $effect(() => {
    const id = instanceId;
    const k = kind;
    // Re-list when a pack is installed/uninstalled from the Browse view (assets
    // have no Tauri events). Read-only here — the bump lives in remove()/update()
    // and in ModBrowseView's handlers; bumping inside this effect would loop.
    void assetsChanged.value;
    const gen = ++generation;
    // Clear any prior update badges + icons — they belonged to the previous list.
    updateStates = new Map();
    summaries = new Map();
    if (id === null) {
      assets = [];
      loading = false;
      error = null;
      return;
    }
    loading = true;
    error = null;
    void (async () => {
      const res = await commands.assetsList(id, k);
      if (gen !== generation) return; // superseded
      if (res.status === 'error') {
        error = formatError(res.error);
        assets = [];
      } else {
        assets = res.data;
        void enrich(res.data, gen);
      }
      loading = false;
    })();
  });

  // Resolve each platform asset's project summary (icon_url) the same way the
  // installed-mods view does — bounded concurrency, guarded against a stale
  // instance/kind switch via the generation counter.
  async function enrich(list: InstalledAsset[], gen: number) {
    const map = new Map<string, ModSummary>();
    await mapLimit(list, 6, async (a) => {
      if (!a.source || !a.project_id) return;
      const p = await commands.modsProject(a.source as ModSource, a.project_id);
      if (p.status === 'ok') map.set(a.filename, p.data.summary);
    });
    if (gen !== generation) return;
    summaries = map;
  }

  async function refresh() {
    if (instanceId === null) return;
    const gen = ++generation;
    const res = await commands.assetsList(instanceId, kind);
    if (gen !== generation) return;
    if (res.status === 'error') error = formatError(res.error);
    else {
      assets = res.data;
      void enrich(res.data, gen);
    }
  }

  function openDetail(asset: InstalledAsset) {
    if (!asset.source || !asset.project_id) return;
    detail = {
      source: asset.source as ModSource,
      projectId: asset.project_id,
      versionId: asset.version_id,
    };
  }

  async function installDetailVersion(v: ModVersion) {
    if (instanceId === null) return;
    detail = null;
    busy = true;
    error = null;
    try {
      const res = await commands.assetInstall(instanceId, v, kind);
      if (res.status === 'error') {
        pushWarning(formatError(res.error));
        return;
      }
      await refresh();
      assetsChanged.value++;
      pushSuccess(get(t)('addons.installed.installedToast', { name: v.name }));
    } finally {
      busy = false;
    }
  }

  async function remove(asset: InstalledAsset) {
    if (instanceId === null) return;
    busy = true;
    error = null;
    try {
      const res = await commands.assetUninstall(instanceId, kind, asset.filename);
      if (res.status === 'error') {
        pushWarning(formatError(res.error));
        return;
      }
      assets = assets.filter((a) => a.filename !== asset.filename);
      const next = new Map(updateStates);
      next.delete(asset.filename);
      updateStates = next;
      // Notify the Browse view so its "Installed" badge clears.
      assetsChanged.value++;
      pushSuccess(get(t)('addons.installed.removedToast', { name: asset.name }));
    } finally {
      busy = false;
    }
  }

  async function checkUpdates() {
    if (instanceId === null) return;
    checking = true;
    error = null;
    try {
      const res = await commands.assetsCheckUpdates(instanceId, kind);
      if (res.status === 'error') {
        pushWarning(formatError(res.error));
        return;
      }
      const map = new Map<string, AssetUpdateState>();
      for (const check of res.data) map.set(check.filename, check.state);
      updateStates = map;
      const anyUpdate = res.data.some((c) => c.state.kind === 'update_available');
      if (!anyUpdate) pushSuccess(get(t)('addons.installed.upToDateToast'));
    } catch (e: unknown) {
      pushWarning(e instanceof Error ? e.message : String(e));
    } finally {
      checking = false;
    }
  }

  async function update(asset: InstalledAsset, latest: ModVersion) {
    if (instanceId === null) return;
    busy = true;
    error = null;
    try {
      const res = await commands.assetInstall(instanceId, latest, kind);
      if (res.status === 'error') {
        pushWarning(formatError(res.error));
        return;
      }
      // The freshly installed version is now current — drop its badge and re-list
      // so name / version_number reflect the new file.
      const next = new Map(updateStates);
      next.delete(asset.filename);
      updateStates = next;
      await refresh();
      // Notify the Browse view so its badge reflects the new version.
      assetsChanged.value++;
      pushSuccess(get(t)('addons.installed.updatedToast', { name: asset.name }));
    } finally {
      busy = false;
    }
  }

  function updatable(filename: string): ModVersion | null {
    const s = updateStates.get(filename);
    return s && s.kind === 'update_available' ? s.latest : null;
  }
  function checkFailed(filename: string): boolean {
    return updateStates.get(filename)?.kind === 'check_failed';
  }
  function checkFailedReason(filename: string): string | null {
    const s = updateStates.get(filename);
    return s?.kind === 'check_failed' ? s.reason : null;
  }
</script>

<div class="p-3">
  <div class="flex items-center justify-end mb-2">
    <BusyButton
      type="button"
      class="btn-secondary btn-sm"
      busy={checking}
      disabled={busy || instanceId === null || assets.length === 0}
      onclick={checkUpdates}
    >
      <Icon name="refresh" class="icon-spin-hover" />
      {$t('addons.installed.checkUpdates')}
    </BusyButton>
  </div>

  {#if error}
    <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2 mb-2">
      {error}
    </div>
  {/if}

  {#if instanceId === null}
    <div class="text-placeholder text-sm py-8 text-center">
      {$t('addons.installed.pickInstance')}
    </div>
  {:else if loading && assets.length === 0}
    <LoadingPanel label={$t('addons.installed.loading')} size="md" />
  {:else if assets.length === 0}
    <div class="text-placeholder text-sm py-8 text-center">{$t('addons.installed.empty')}</div>
  {:else}
    <div class="border border-border-subtle rounded-lg overflow-hidden">
      {#each assets as asset (asset.filename)}
        {@const latest = updatable(asset.filename)}
        <CardShell variant="compact-row" accent={latest ? 'warning' : 'none'}>
          {#if asset.source && asset.project_id}
            <button
              type="button"
              class="flex flex-1 items-center gap-2.5 min-w-0 text-left"
              onclick={() => openDetail(asset)}
            >
              <CardMedia
                iconUrl={summaries.get(asset.filename)?.icon_url ?? null}
                placeholder={kind === 'shader' ? 'shader' : 'resourcePack'}
                size="sm"
              />
              <span class="min-w-0 flex-1 truncate">
                <span class="text-sm text-primary">{asset.name}</span>
                {#if asset.version_number}
                  <span class="text-xs text-muted ml-2">{asset.version_number}</span>
                {/if}
              </span>
            </button>
          {:else}
            <CardMedia
              iconUrl={null}
              placeholder={kind === 'shader' ? 'shader' : 'resourcePack'}
              size="sm"
            />
            <div class="min-w-0 flex-1 truncate">
              <span class="text-sm text-primary">{asset.name}</span>
              {#if asset.version_number}
                <span class="text-xs text-muted ml-2">{asset.version_number}</span>
              {/if}
            </div>
          {/if}
          {#if checkFailed(asset.filename)}
            {@const reason = checkFailedReason(asset.filename)}
            <span
              class="text-warning-text flex-shrink-0"
              use:tooltip={reason ?? $t('addons.installed.checkFailed')}
              aria-label={$t('addons.installed.checkFailed')}
              role="img"
            >
              <Icon name="warning" size={15} />
            </span>
          {/if}
          {#if latest}
            <button
              type="button"
              class="btn-icon btn-icon-sm btn-icon-warning"
              disabled={busy}
              onclick={() => update(asset, latest)}
              aria-label={$t('addons.installed.update')}
              use:tooltip={$t('addons.installed.update')}><Icon name="refresh" size={15} /></button
            >
          {/if}
          <button
            type="button"
            class="btn-icon btn-icon-sm btn-icon-danger"
            disabled={busy}
            onclick={() => remove(asset)}
            aria-label={$t('addons.installed.remove')}
            use:tooltip={$t('addons.installed.remove')}><Icon name="trash" size={15} /></button
          >
        </CardShell>
      {/each}
    </div>
  {/if}

  {#if detail && instanceId}
    <ModDetailModal
      source={detail.source}
      projectId={detail.projectId}
      {mcVersion}
      {loader}
      installedVersionId={detail.versionId}
      onClose={() => (detail = null)}
      onInstall={installDetailVersion}
    />
  {/if}
</div>
