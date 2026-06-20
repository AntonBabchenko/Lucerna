<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import { formatError } from '$lib/ipc/format-error';
  import { relativeTime } from '$lib/format/relative-time';
  import { formatSize } from '$lib/format/size';
  import { serverState } from '$lib/servers/server-state.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import type { BackupInfo } from '$lib/ipc/bindings';

  const KEEP = 10;

  let { serverId }: { serverId: string } = $props();

  let backups = $state<BackupInfo[]>([]);
  let listError = $state<string | null>(null);
  let busyCreate = $state(false);
  let createError = $state<string | null>(null);

  // Per-row confirm state: 'restore' | 'delete' | null
  let confirmFor = $state<{ file_name: string; kind: 'restore' | 'delete' } | null>(null);
  let busyAction = $state<string | null>(null); // file_name being actioned
  let actionError = $state<string | null>(null);

  const running = $derived(serverState.running(serverId));

  async function reload() {
    listError = null;
    const r = await serverState.backupList(serverId);
    if (r.ok) {
      backups = r.list ?? [];
    } else {
      listError = formatError(r.error as Parameters<typeof formatError>[0]);
    }
  }

  onMount(() => {
    void reload();
  });

  async function create() {
    busyCreate = true;
    createError = null;
    try {
      const r = await serverState.backupCreate(serverId);
      if (r.ok) {
        await reload();
      } else {
        createError = formatError(r.error as Parameters<typeof formatError>[0]);
      }
    } finally {
      busyCreate = false;
    }
  }

  function startConfirm(file_name: string, kind: 'restore' | 'delete') {
    confirmFor = { file_name, kind };
    actionError = null;
  }

  function cancelConfirm() {
    confirmFor = null;
  }

  async function confirmAction() {
    if (!confirmFor) return;
    const { file_name, kind } = confirmFor;
    confirmFor = null;
    busyAction = file_name;
    actionError = null;
    try {
      if (kind === 'restore') {
        const r = await serverState.backupRestore(serverId, file_name);
        if (!r.ok) {
          actionError = formatError(r.error as Parameters<typeof formatError>[0]);
        } else {
          await reload();
        }
      } else {
        const r = await serverState.backupDelete(serverId, file_name);
        if (!r.ok) {
          actionError = formatError(r.error as Parameters<typeof formatError>[0]);
        } else {
          await reload();
        }
      }
    } finally {
      busyAction = null;
    }
  }
</script>

<div class="flex flex-col gap-4">
  <!-- Header row: Create backup button + note -->
  <div class="flex items-center justify-between gap-3">
    <p class="text-xs text-muted">{$t('servers.backups.note', { n: KEEP })}</p>
    <BusyButton class="btn-primary btn-sm" busy={busyCreate} onclick={() => void create()}>
      {busyCreate ? $t('servers.backups.creating') : $t('servers.backups.create')}
    </BusyButton>
  </div>

  {#if createError}
    <p class="text-sm text-danger">{createError}</p>
  {/if}

  {#if actionError}
    <p class="text-sm text-danger">{actionError}</p>
  {/if}

  {#if running}
    <p class="text-xs text-warning">{$t('servers.backups.runningBlock')}</p>
  {/if}

  {#if listError}
    <p class="text-sm text-danger">{listError}</p>
  {:else if backups.length === 0}
    <p class="text-sm text-muted py-6 text-center">{$t('servers.backups.empty')}</p>
  {:else}
    <ul class="flex flex-col gap-1">
      {#each backups as backup (backup.file_name)}
        <li
          class="flex items-center gap-3 rounded-md border border-border-subtle px-3 py-2 text-sm"
        >
          <span class="flex-1 truncate font-mono text-xs text-primary">{backup.file_name}</span>
          <span class="text-muted text-xs shrink-0">
            {relativeTime($t, backup.created_unix_ms ?? 0)}
          </span>
          <span class="text-muted text-xs shrink-0">
            {formatSize($t, backup.size_bytes)}
          </span>

          <!-- Row actions or inline confirm -->
          {#if confirmFor?.file_name === backup.file_name}
            <span class="flex items-center gap-1.5 shrink-0">
              <span class="text-xs text-secondary">
                {confirmFor.kind === 'restore'
                  ? $t('servers.backups.restoreConfirm', { name: backup.file_name })
                  : $t('servers.backups.deleteConfirm', { name: backup.file_name })}
              </span>
              <button
                type="button"
                data-confirm
                class={confirmFor.kind === 'restore' ? 'btn-primary btn-xs' : 'btn-danger btn-xs'}
                onclick={() => void confirmAction()}
              >
                {confirmFor.kind === 'restore'
                  ? $t('servers.backups.restore')
                  : $t('servers.backups.delete')}
              </button>
              <button type="button" class="btn-ghost btn-xs" onclick={cancelConfirm}>
                {$t('common.cancel')}
              </button>
            </span>
          {:else}
            <span class="flex items-center gap-1.5 shrink-0">
              <button
                type="button"
                class="btn-secondary btn-xs"
                disabled={running || busyAction === backup.file_name}
                title={running ? $t('servers.backups.runningBlock') : undefined}
                onclick={() => startConfirm(backup.file_name, 'restore')}
              >
                {$t('servers.backups.restore')}
              </button>
              <button
                type="button"
                class="btn-ghost btn-xs text-danger hover:bg-danger/10"
                disabled={busyAction === backup.file_name}
                onclick={() => startConfirm(backup.file_name, 'delete')}
              >
                {$t('servers.backups.delete')}
              </button>
            </span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>
