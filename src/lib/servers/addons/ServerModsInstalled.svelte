<script lang="ts">
  import { get } from 'svelte/store';
  import { commands, type ServerModEntry } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { modCapable, pluginCapable } from '$lib/servers/core-display';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import CardShell from '$lib/ui/cards/CardShell.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';

  let { serverId, reloadToken = 0 }: { serverId: string; reloadToken?: number } = $props();

  let mods = $state<ServerModEntry[]>([]);
  let loadError = $state<string | null>(null);
  let actionError = $state<string | null>(null);
  let busyDelete = $state<string | null>(null);
  let busyRestore = $state<string | null>(null);
  let busyFolder = $state(false);
  let busyQuarantine = $state(false);
  let pendingDelete = $state<string | null>(null);

  // The server's own metadata drives mod applicability. Mods only attach to a
  // mod loader; a vanilla server gets datapacks only. Plugin cores (paper/purpur)
  // have no mod loader either — they get datapacks + plugins, not mods.
  // Mutations require a stopped server (the backend enforces it; the UI
  // gates to avoid pointless errors).
  const server = $derived(serverState.list.find((s) => s.id === serverId) ?? null);
  const isModCapable = $derived(server !== null && modCapable(server.loader));
  const isPluginCore = $derived(server !== null && pluginCapable(server.loader));
  const isRunning = $derived(server?.running ?? false);
  const canManageMods = $derived(server !== null && isModCapable && !isRunning);

  async function refresh() {
    const res = await commands.serverListMods(serverId);
    if (res.status === 'ok') {
      mods = res.data;
      loadError = null;
    } else {
      loadError = formatError(res.error);
    }
  }

  // Re-read on mount and whenever the Add-ons host signals a mutation
  // (dropzone/browser install while this pane is visible).
  $effect(() => {
    const _ = reloadToken;
    void refresh();
  });

  async function quarantineClientMods() {
    busyQuarantine = true;
    actionError = null;
    try {
      const r = await serverState.quarantineClientMods(serverId);
      if (r.ok) {
        const n = r.report?.disabled.length ?? 0;
        const kept = r.report?.kept_because_required.length ?? 0;
        let msg =
          n > 0
            ? get(t)('servers.diagnose.quarantined', { count: n })
            : get(t)('servers.diagnose.quarantineNone');
        if (kept > 0) {
          msg += ` ${get(t)('servers.diagnose.quarantineKeptRequired', { count: kept })}`;
        }
        pushSuccess(msg);
        await refresh();
      } else {
        actionError = formatError(r.error as Parameters<typeof formatError>[0]);
      }
    } finally {
      busyQuarantine = false;
    }
  }

  async function restore(filename: string) {
    busyRestore = filename;
    actionError = null;
    try {
      const res = await commands.serverEnableMod(serverId, filename);
      if (res.status === 'ok') {
        await refresh();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      busyRestore = null;
    }
  }

  function requestDelete(filename: string) {
    pendingDelete = filename;
    actionError = null;
  }

  async function confirmDelete(filename: string) {
    busyDelete = filename;
    actionError = null;
    try {
      const res = await commands.serverDeleteMod(serverId, filename);
      if (res.status === 'ok') {
        pendingDelete = null;
        await refresh();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      busyDelete = null;
    }
  }

  async function openFolder() {
    busyFolder = true;
    try {
      const res = await commands.serverOpenFolder(serverId);
      if (res.status !== 'ok') {
        actionError = formatError(res.error);
      }
    } finally {
      busyFolder = false;
    }
  }
</script>

<div class="flex flex-col gap-3">
  <!-- Toolbar: folder + quarantine only — local/browser installs are owned by
       the Add-ons host (tab-level dropzone + Browse sub-tab). -->
  <div class="flex flex-wrap items-center gap-2">
    {#if !isPluginCore}
      <BusyButton class="btn-secondary btn-sm" busy={busyFolder} onclick={() => void openFolder()}>
        <Icon name="folderOpen" size={14} />
        {$t('servers.mods.openFolder')}
      </BusyButton>
    {/if}
    {#if isModCapable}
      <BusyButton
        class="btn-secondary btn-sm"
        data-testid="server-mods-quarantine"
        busy={busyQuarantine}
        disabled={!canManageMods}
        onclick={() => void quarantineClientMods()}
      >
        {$t('servers.diagnose.quarantineClientMods')}
      </BusyButton>
    {/if}
  </div>

  {#if isPluginCore}
    <p class="text-xs text-secondary">{$t('servers.mods.pluginCoreHint')}</p>
  {:else}
    {#if isRunning}
      <p class="text-xs text-warning-text">{$t('servers.mods.stopToManage')}</p>
    {/if}

    <!-- Note -->
    <p class="text-xs text-secondary">{$t('servers.mods.note')}</p>

    {#if loadError}
      <p class="text-sm text-danger">{loadError}</p>
    {/if}
    {#if actionError}
      <p class="text-sm text-danger">{actionError}</p>
    {/if}

    {#if mods.length === 0 && !loadError}
      <p class="text-sm text-muted">{$t('servers.mods.empty')}</p>
    {:else}
      <div class="overflow-hidden rounded-lg border border-border-subtle">
        {#each mods as entry (entry.filename)}
          <CardShell variant="compact-row" dim={entry.disabled}>
            <span class="flex-1 truncate font-mono text-xs text-primary">{entry.filename}</span>

            {#if entry.disabled}
              <StatusBadge variant="muted">
                {entry.reason === 'client_only'
                  ? $t('servers.mods.setAsideClientOnly')
                  : $t('servers.mods.setAside')}
              </StatusBadge>
            {/if}

            {#if pendingDelete === entry.filename}
              <!-- Inline confirm row -->
              <span class="text-xs text-secondary shrink-0">
                {$t('servers.mods.deleteConfirm', { name: entry.filename })}
              </span>
              <BusyButton
                class="btn-danger btn-xs"
                busy={busyDelete === entry.filename}
                onclick={() => void confirmDelete(entry.filename)}
              >
                {$t('servers.mods.delete')}
              </BusyButton>
              <button
                type="button"
                class="btn-ghost btn-xs"
                disabled={busyDelete === entry.filename}
                onclick={() => (pendingDelete = null)}
              >
                {$t('common.cancel')}
              </button>
            {:else}
              {#if entry.disabled && canManageMods}
                <BusyButton
                  class="btn-ghost btn-xs inline-flex items-center gap-1"
                  busy={busyRestore === entry.filename}
                  onclick={() => void restore(entry.filename)}
                  data-testid="server-mod-restore"
                >
                  <Icon name="restore" size={13} />
                  {$t('servers.mods.restore')}
                </BusyButton>
              {/if}
              {#if isRunning}
                <span use:tooltip={{ text: $t('servers.mods.stopToManage'), describe: false }}>
                  <button
                    type="button"
                    class="btn-icon btn-icon-sm btn-icon-danger"
                    aria-label={$t('servers.mods.delete')}
                    disabled
                  >
                    <Icon name="trash" size={13} />
                  </button>
                </span>
              {:else}
                <button
                  type="button"
                  class="btn-icon btn-icon-sm btn-icon-danger"
                  aria-label={$t('servers.mods.delete')}
                  use:tooltip={$t('servers.mods.delete')}
                  onclick={() => requestDelete(entry.filename)}
                >
                  <Icon name="trash" size={13} />
                </button>
              {/if}
            {/if}
          </CardShell>
        {/each}
      </div>
    {/if}
  {/if}
</div>
