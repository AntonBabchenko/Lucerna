<script lang="ts">
  import { commands, type Backup, type World } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import RestoreBackupDialog from '$lib/worlds/RestoreBackupDialog.svelte';
  import { t, locale } from '$lib/i18n';
  import { formatSize } from '$lib/format/size';
  import Modal from '$lib/ui/Modal.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { get } from 'svelte/store';

  let {
    instanceId,
    world,
    onClose,
    onChanged,
  }: {
    instanceId: string;
    world: World;
    onClose: () => void;
    onChanged: () => void;
  } = $props();

  let backups = $state<Backup[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let backingUp = $state(false);
  let restoreFor = $state<Backup | null>(null);

  async function reload() {
    loading = true;
    error = null;
    const r = await commands.listBackups(instanceId, world.folder_name);
    loading = false;
    if (r.status === 'ok') backups = r.data;
    else error = formatError(r.error);
  }

  $effect(() => void reload());

  async function onDelete(b: Backup) {
    if (!confirm(get(t)('worlds.backups.confirmDelete', { timestamp: formatBackupTimestamp(b) })))
      return;
    const r = await commands.deleteBackup(instanceId, world.folder_name, b.filename);
    if (r.status === 'ok') {
      onChanged();
      await reload();
    } else {
      error = formatError(r.error);
    }
  }

  async function onBackupNow() {
    backingUp = true;
    error = null;
    const r = await commands.backupWorld(instanceId, world.folder_name);
    backingUp = false;
    if (r.status === 'ok') {
      onChanged();
      await reload();
    } else {
      error = formatError(r.error);
    }
  }

  async function onOpenBackupsFolder() {
    const r = await commands.openBackupsFolder(instanceId, world.folder_name);
    if (r.status !== 'ok') error = formatError(r.error);
  }

  function formatBackupTimestamp(b: Backup): string {
    if (!b.created_unix_ms) return b.filename;
    return new Date(b.created_unix_ms).toLocaleString($locale ?? undefined, {
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
  }

  const totalSize = $derived(backups.reduce((a, b) => a + (b.size_bytes ?? 0), 0));
</script>

<Modal ariaLabelledby="backups-dialog-title" {onClose} panelClass="max-w-lg w-full p-4">
  <div class="flex items-center justify-between gap-2 mb-3">
    <h3 id="backups-dialog-title" class="font-semibold text-lg text-primary">
      {$t('worlds.backups.title', { world: world.folder_name })}
    </h3>
    <BusyButton
      type="button"
      class="btn-secondary btn-sm inline-flex items-center gap-1 flex-shrink-0"
      data-testid="backups-create-btn"
      busy={backingUp}
      onclick={() => void onBackupNow()}
    >
      <Icon name="archive" size={14} />
      {$t('worlds.backups.backupNow')}
    </BusyButton>
  </div>
  {#if loading}
    <LoadingPanel label={$t('worlds.backups.loading')} />
  {:else if error}
    <p class="text-sm text-danger mb-2">{error}</p>
  {:else if backups.length === 0}
    <p class="text-sm text-muted">
      {$t('worlds.backups.empty')}
    </p>
  {:else}
    <ul
      class="border border-border-subtle rounded divide-y divide-border-subtle mb-3 max-h-80 overflow-auto"
    >
      {#each backups as b (b.filename)}
        <li class="flex items-center justify-between gap-2 px-3 py-2 hover:bg-subtle">
          <div class="min-w-0">
            <div class="text-sm font-medium">{formatBackupTimestamp(b)}</div>
            <div class="text-xs text-muted">{formatSize($t, b.size_bytes)}</div>
          </div>
          <div class="flex items-center gap-1 flex-shrink-0">
            <button
              type="button"
              class="btn-icon btn-icon-sm"
              data-testid="backup-restore-btn"
              aria-label={$t('worlds.backups.restore')}
              use:tooltip={$t('worlds.backups.restore')}
              onclick={() => (restoreFor = b)}
            >
              <Icon name="restore" size={15} />
            </button>
            <button
              type="button"
              class="btn-icon btn-icon-sm btn-icon-danger"
              data-testid="backup-delete-btn"
              aria-label={$t('worlds.backups.deleteBackup')}
              use:tooltip={$t('worlds.backups.deleteBackup')}
              onclick={() => void onDelete(b)}
            >
              <Icon name="trash" size={15} />
            </button>
          </div>
        </li>
      {/each}
    </ul>
    <div class="text-xs text-muted mb-3 flex justify-between">
      <span>{$t('worlds.backups.total', { size: formatSize($t, totalSize) })}</span>
      <button
        type="button"
        class="btn-tertiary inline-flex items-center gap-1"
        onclick={() => void onOpenBackupsFolder()}
      >
        {$t('worlds.backups.openBackupsFolder')}
        <Icon name="folderOpen" size={14} />
      </button>
    </div>
  {/if}
  <div class="flex justify-end">
    <button type="button" class="btn-secondary btn-sm" onclick={onClose}
      >{$t('common.close')}</button
    >
  </div>
</Modal>

{#if restoreFor}
  <RestoreBackupDialog
    {instanceId}
    worldFolder={world.folder_name}
    backup={restoreFor}
    onClose={() => (restoreFor = null)}
    onRestored={() => {
      restoreFor = null;
      onChanged();
      void reload();
      onClose();
    }}
  />
{/if}
