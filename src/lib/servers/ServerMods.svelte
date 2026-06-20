<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { Icon } from '$lib/ui/icons';

  let { serverId }: { serverId: string } = $props();

  let mods = $state<string[]>([]);
  let loadError = $state<string | null>(null);
  let deleteError = $state<string | null>(null);
  let busyDelete = $state<string | null>(null); // filename currently being deleted
  let busyFolder = $state(false);
  let busyQuarantine = $state(false);
  // Per-row inline confirm: filename awaiting confirmation, or null
  let pendingDelete = $state<string | null>(null);

  // A jar set aside (renamed to `*.jar.disabled`) — shown muted, not loadable.
  function isDisabled(filename: string): boolean {
    return filename.toLowerCase().endsWith('.jar.disabled');
  }

  // Proactively set aside client-only mods (metadata + offline env, dep-safe).
  async function quarantineClientMods() {
    busyQuarantine = true;
    deleteError = null;
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
        deleteError = formatError(r.error as Parameters<typeof formatError>[0]);
      }
    } finally {
      busyQuarantine = false;
    }
  }

  async function refresh() {
    const res = await commands.serverListMods(serverId);
    if (res.status === 'ok') {
      mods = res.data;
    } else {
      loadError = formatError(res.error);
    }
  }

  onMount(() => {
    void refresh();
  });

  function requestDelete(filename: string) {
    pendingDelete = filename;
    deleteError = null;
  }

  function cancelDelete() {
    pendingDelete = null;
  }

  async function confirmDelete(filename: string) {
    busyDelete = filename;
    deleteError = null;
    try {
      const res = await commands.serverDeleteMod(serverId, filename);
      if (res.status === 'ok') {
        pendingDelete = null;
        await refresh();
      } else {
        deleteError = formatError(res.error);
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
        deleteError = formatError(res.error);
      }
    } finally {
      busyFolder = false;
    }
  }
</script>

<div class="flex flex-col gap-3">
  <!-- Toolbar -->
  <div class="flex items-center gap-2">
    <BusyButton class="btn-secondary btn-sm" busy={busyFolder} onclick={() => void openFolder()}>
      <Icon name="folderOpen" size={14} />
      {$t('servers.mods.openFolder')}
    </BusyButton>
    <BusyButton
      class="btn-secondary btn-sm"
      data-testid="server-mods-quarantine"
      busy={busyQuarantine}
      onclick={() => void quarantineClientMods()}
    >
      {$t('servers.diagnose.quarantineClientMods')}
    </BusyButton>
  </div>

  <!-- Note -->
  <p class="text-xs text-secondary">{$t('servers.mods.note')}</p>

  {#if loadError}
    <p class="text-sm text-danger">{loadError}</p>
  {/if}

  {#if deleteError}
    <p class="text-sm text-danger">{deleteError}</p>
  {/if}

  {#if mods.length === 0 && !loadError}
    <p class="text-sm text-muted">{$t('servers.mods.empty')}</p>
  {:else}
    <ul class="flex flex-col divide-y divide-border-subtle rounded border border-border-subtle">
      {#each mods as filename (filename)}
        <li class="flex items-center gap-2 px-3 py-2 text-sm">
          <span
            class="flex-1 truncate font-mono text-xs {isDisabled(filename)
              ? 'text-muted line-through'
              : 'text-primary'}">{filename}</span
          >
          {#if isDisabled(filename)}
            <span class="shrink-0 text-xs text-muted">{$t('servers.mods.setAside')}</span>
          {/if}

          {#if pendingDelete === filename}
            <!-- Inline confirm row -->
            <span class="text-xs text-secondary shrink-0">
              {$t('servers.mods.deleteConfirm', { name: filename })}
            </span>
            <BusyButton
              class="btn-danger btn-xs"
              busy={busyDelete === filename}
              onclick={() => void confirmDelete(filename)}
            >
              {$t('servers.mods.delete')}
            </BusyButton>
            <button
              type="button"
              class="btn-ghost btn-xs"
              disabled={busyDelete === filename}
              onclick={cancelDelete}
            >
              {$t('common.cancel')}
            </button>
          {:else}
            <button
              type="button"
              class="btn-ghost btn-xs"
              title={$t('servers.mods.delete')}
              onclick={() => requestDelete(filename)}
            >
              <Icon name="trash" size={13} />
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>
