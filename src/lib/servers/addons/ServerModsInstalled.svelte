<script lang="ts">
  import { get } from 'svelte/store';
  import { onDestroy } from 'svelte';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { modCapable, pluginCapable } from '$lib/servers/core-display';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import ServerInstalledRow from './ServerInstalledRow.svelte';
  import { createServerInstalledData, type ServerRow } from './server-installed-data.svelte';

  let { serverId, reloadToken = 0 }: { serverId: string; reloadToken?: number } = $props();

  // Enriched Installed list (enrich → ModSummary resolution → ServerRow[]). The
  // composable owns the list + reload-token effect and blanks on server switch.
  const data = createServerInstalledData(
    () => serverId,
    'mod',
    () => reloadToken,
  );
  onDestroy(() => data.dispose());

  let actionError = $state<string | null>(null);
  let busyFolder = $state(false);
  let busyQuarantine = $state(false);
  // Pane-level delete confirm (ServerInstalledRow delegates delete up).
  let pendingDelete = $state<ServerRow | null>(null);
  let deleting = $state(false);

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

  // Toggle enable/disable — MUST use `on_disk_filename` (a disabled mod lives at
  // `<name>.jar.disabled`), never the base display filename.
  async function toggle(row: ServerRow) {
    actionError = null;
    const res = row.card.installed.enabled
      ? await commands.serverDisableMod(serverId, row.onDiskFilename)
      : await commands.serverEnableMod(serverId, row.onDiskFilename);
    if (res.status === 'ok') await data.refresh();
    else actionError = formatError(res.error);
  }

  async function confirmDelete(row: ServerRow) {
    actionError = null;
    deleting = true;
    try {
      const res = await commands.serverDeleteMod(serverId, row.onDiskFilename);
      if (res.status === 'ok') {
        pendingDelete = null;
        await data.refresh();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      deleting = false;
    }
  }

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
        await data.refresh();
      } else {
        actionError = formatError(r.error as Parameters<typeof formatError>[0]);
      }
    } finally {
      busyQuarantine = false;
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

    {#if data.error}
      <p class="text-sm text-danger">{data.error}</p>
    {/if}
    {#if actionError}
      <p class="text-sm text-danger">{actionError}</p>
    {/if}

    {#if data.rows.length === 0 && !data.error}
      <p class="text-sm text-muted">{$t('servers.mods.empty')}</p>
    {:else}
      <div class="flex flex-col gap-2">
        {#each data.rows as row (row.sha1)}
          <ServerInstalledRow
            card={row.card}
            reason={row.reason}
            canToggle={canManageMods}
            onToggle={() => void toggle(row)}
            onUninstall={() => {
              actionError = null;
              pendingDelete = row;
            }}
          />
        {/each}
      </div>
    {/if}

    {#if pendingDelete}
      <ConfirmDialog
        title={$t('servers.mods.delete')}
        bodyText={$t('servers.mods.deleteConfirm', { name: pendingDelete.card.installed.name })}
        confirmLabel={$t('servers.mods.delete')}
        variant="danger"
        busy={deleting}
        error={actionError}
        onCancel={() => (pendingDelete = null)}
        onConfirm={() => pendingDelete && void confirmDelete(pendingDelete)}
      />
    {/if}
  {/if}
</div>
