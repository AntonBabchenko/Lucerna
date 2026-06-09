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
    type ModVersion,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { assetsChanged } from '$lib/settings/state.svelte';
  import { t } from '$lib/i18n';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { get } from 'svelte/store';

  let {
    instanceId,
    kind,
  }: {
    instanceId: string | null;
    kind: ContentKind;
  } = $props();

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
    // Clear any prior update badges — they belonged to the previous list.
    updateStates = new Map();
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
      }
      loading = false;
    })();
  });

  async function refresh() {
    if (instanceId === null) return;
    const gen = ++generation;
    const res = await commands.assetsList(instanceId, kind);
    if (gen !== generation) return;
    if (res.status === 'error') error = formatError(res.error);
    else assets = res.data;
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
      pushSuccess(asset.name);
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
      pushSuccess(asset.name);
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
    <div class="text-placeholder text-sm py-8 text-center">
      {$t('addons.installed.loading')}
    </div>
  {:else if assets.length === 0}
    <div class="text-placeholder text-sm py-8 text-center">{$t('addons.installed.empty')}</div>
  {:else}
    <div class="border border-border-subtle rounded overflow-hidden">
      {#each assets as asset (asset.filename)}
        {@const latest = updatable(asset.filename)}
        <div
          class="flex items-center gap-3 px-3 py-2 border-b border-border-subtle last:border-b-0"
        >
          <div class="min-w-0 flex-1">
            <div class="text-sm text-primary truncate">{asset.name}</div>
            {#if asset.version_number}
              <div class="text-xs text-secondary truncate">{asset.version_number}</div>
            {/if}
          </div>
          {#if checkFailed(asset.filename)}
            {@const reason = checkFailedReason(asset.filename)}
            <span
              class="text-xs text-placeholder"
              title={reason ?? $t('addons.installed.checkFailed')}
              aria-label={$t('addons.installed.checkFailed')}
              role="img"
            >
              ⚠
            </span>
          {/if}
          {#if latest}
            <BusyButton
              type="button"
              class="btn-primary btn-sm"
              {busy}
              onclick={() => update(asset, latest)}
            >
              {$t('addons.installed.update')}
            </BusyButton>
          {/if}
          <BusyButton
            type="button"
            class="btn-secondary btn-sm"
            {busy}
            onclick={() => remove(asset)}
          >
            {$t('addons.installed.remove')}
          </BusyButton>
        </div>
      {/each}
    </div>
  {/if}
</div>
