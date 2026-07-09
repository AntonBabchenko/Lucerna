<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { commands, type ServerPluginEntry } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { pluginCapable } from '$lib/servers/core-display';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import ServerPluginBrowser from './plugins/ServerPluginBrowser.svelte';
  import CardShell from '$lib/ui/cards/CardShell.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';

  let { serverId }: { serverId: string } = $props();

  let plugins = $state<ServerPluginEntry[]>([]);
  let loadError = $state<string | null>(null);
  let actionError = $state<string | null>(null);
  let busyDelete = $state<string | null>(null);
  let busyRestore = $state<string | null>(null);
  let busyDisable = $state<string | null>(null);
  let busyFolder = $state(false);
  let busyLocal = $state(false);
  let showBrowser = $state(false);
  let pendingDelete = $state<string | null>(null);

  // Plugins only attach to plugin-capable cores (paper/purpur) — a mod-loader
  // or vanilla server gets no Plugins UI, just the requiresCore hint below.
  // Mutations require a stopped server (the backend enforces it; the UI gates
  // to avoid pointless errors).
  const server = $derived(serverState.list.find((s) => s.id === serverId) ?? null);
  const isPluginCore = $derived(server !== null && pluginCapable(server.loader));
  const isRunning = $derived(server?.running ?? false);
  const canManage = $derived(isPluginCore && !isRunning);

  async function refresh() {
    const res = await commands.serverListPlugins(serverId);
    if (res.status === 'ok') {
      plugins = res.data;
      loadError = null;
    } else {
      loadError = formatError(res.error);
    }
  }

  onMount(() => {
    void refresh();
  });

  async function installLocal() {
    actionError = null;
    const picked = await openFile({
      multiple: false,
      filters: [{ name: get(t)('common.fileFilter.modJar'), extensions: ['jar'] }],
    });
    if (typeof picked !== 'string') return;
    busyLocal = true;
    try {
      const res = await commands.serverInstallPluginLocal(serverId, picked);
      if (res.status === 'ok') {
        pushSuccess(get(t)('servers.plugins.localInstalled', { name: res.data }));
        await refresh();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      busyLocal = false;
    }
  }

  async function restore(filename: string) {
    busyRestore = filename;
    actionError = null;
    try {
      const res = await commands.serverEnablePlugin(serverId, filename);
      if (res.status === 'ok') {
        await refresh();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      busyRestore = null;
    }
  }

  async function disable(filename: string) {
    busyDisable = filename;
    actionError = null;
    try {
      const res = await commands.serverDisablePlugin(serverId, filename);
      if (res.status === 'ok') {
        await refresh();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      busyDisable = null;
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
      const res = await commands.serverDeletePlugin(serverId, filename);
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
      const res = await commands.serverOpenPluginsFolder(serverId);
      if (res.status !== 'ok') {
        actionError = formatError(res.error);
      }
    } finally {
      busyFolder = false;
    }
  }
</script>

{#if !isPluginCore}
  <p class="text-sm text-secondary">{$t('servers.plugins.requiresCore')}</p>
{:else}
  <div class="flex flex-col gap-3">
    <!-- Toolbar -->
    <div class="flex flex-wrap items-center gap-2">
      <BusyButton class="btn-secondary btn-sm" busy={busyFolder} onclick={() => void openFolder()}>
        <Icon name="folderOpen" size={14} />
        {$t('servers.plugins.openFolder')}
      </BusyButton>
      <button
        type="button"
        class="btn-secondary btn-sm inline-flex items-center gap-1"
        disabled={!canManage}
        onclick={() => (showBrowser = !showBrowser)}
        data-testid="server-plugins-add"
      >
        <Icon name="plus" size={14} />
        {$t('servers.plugins.addPlugins')}
      </button>
      <BusyButton
        class="btn-secondary btn-sm"
        busy={busyLocal}
        disabled={!canManage}
        onclick={() => void installLocal()}
        data-testid="server-plugins-install-local"
      >
        <Icon name="upload" size={14} />
        {$t('servers.plugins.installLocal')}
      </BusyButton>
    </div>

    {#if isRunning}
      <p class="text-xs text-warning-text">{$t('servers.plugins.stopToManage')}</p>
    {/if}

    <!-- Server-targeted plugin browser (collapsible). The bare `server &&` is
         for TS narrowing only — canManage already implies server is non-null. -->
    {#if showBrowser && canManage && server}
      <div class="rounded border border-border-subtle p-2">
        <ServerPluginBrowser
          {serverId}
          mcVersion={server.mc_version}
          core={server.loader}
          onInstalled={() => void refresh()}
        />
      </div>
    {/if}

    <!-- Note -->
    <p class="text-xs text-secondary">{$t('servers.plugins.note')}</p>

    {#if loadError}
      <p class="text-sm text-danger">{loadError}</p>
    {/if}
    {#if actionError}
      <p class="text-sm text-danger">{actionError}</p>
    {/if}

    {#if plugins.length === 0 && !loadError}
      <p class="text-sm text-muted">{$t('servers.plugins.empty')}</p>
    {:else}
      <div class="overflow-hidden rounded-lg border border-border-subtle">
        {#each plugins as entry (entry.filename)}
          <CardShell variant="compact-row" dim={entry.disabled}>
            <span class="flex-1 truncate font-mono text-xs text-primary">{entry.filename}</span>

            {#if entry.disabled}
              <StatusBadge variant="muted">
                {$t('servers.plugins.setAside')}
              </StatusBadge>
            {/if}

            {#if pendingDelete === entry.filename}
              <!-- Inline confirm row -->
              <span class="text-xs text-secondary shrink-0">
                {$t('servers.plugins.deleteConfirm', { name: entry.filename })}
              </span>
              <BusyButton
                class="btn-danger btn-xs"
                busy={busyDelete === entry.filename}
                onclick={() => void confirmDelete(entry.filename)}
              >
                {$t('servers.plugins.delete')}
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
              {#if entry.disabled && canManage}
                <BusyButton
                  class="btn-ghost btn-xs inline-flex items-center gap-1"
                  busy={busyRestore === entry.filename}
                  onclick={() => void restore(entry.filename)}
                  data-testid="server-plugin-restore"
                >
                  <Icon name="restore" size={13} />
                  {$t('servers.plugins.restore')}
                </BusyButton>
              {:else if !entry.disabled && canManage}
                <BusyButton
                  class="btn-ghost btn-xs"
                  busy={busyDisable === entry.filename}
                  onclick={() => void disable(entry.filename)}
                  data-testid="server-plugin-disable"
                >
                  {$t('servers.plugins.disable')}
                </BusyButton>
              {/if}
              {#if isRunning}
                <span use:tooltip={{ text: $t('servers.plugins.stopToManage'), describe: false }}>
                  <button
                    type="button"
                    class="btn-icon btn-icon-sm btn-icon-danger"
                    aria-label={$t('servers.plugins.delete')}
                    disabled
                  >
                    <Icon name="trash" size={13} />
                  </button>
                </span>
              {:else}
                <button
                  type="button"
                  class="btn-icon btn-icon-sm btn-icon-danger"
                  aria-label={$t('servers.plugins.delete')}
                  use:tooltip={$t('servers.plugins.delete')}
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
  </div>
{/if}
