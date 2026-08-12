<script lang="ts">
  import { commands, type Backup, type RestoreMode } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import RestoreBackupDialog from '$lib/worlds/RestoreBackupDialog.svelte';
  import { t, locale } from '$lib/i18n';
  import { formatSize } from '$lib/format/size';
  import Modal from '$lib/ui/Modal.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import BusyButton from '$lib/ui/BusyButton.svelte';

  // Body content for the Backups tab of WorldDetailDialog. Used to live in
  // BackupsDialog, which owned its own Modal — now retired in favour of the
  // shared tabbed dialog (see WorldDetailDialog.svelte), so this panel owns
  // no chrome (no Modal, no title heading, no Close button): the caller's
  // Modal supplies the title and the close affordance. Owns all backup
  // state/actions itself so the caller stays a thin shell.

  // Takes the folder NAME, not a `World`: it only ever needed `folder_name`, and
  // a backup set whose world is gone has no `World` to pass. That is what lets
  // the orphaned-backups section mount this panel unchanged.
  let {
    instanceId,
    worldFolder,
    onClose,
    onChanged,
    lockedMode,
    modeNote,
  }: {
    instanceId: string;
    worldFolder: string;
    onClose: () => void;
    onChanged: () => void;
    /// When set, the restore dialog offers no choice — Replace calls
    /// `world_dir_at` and would fail `WorldNotFound` for an orphaned set.
    lockedMode?: RestoreMode;
    modeNote?: string;
  } = $props();

  let backups = $state<Backup[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let backingUp = $state(false);
  let restoreFor = $state<Backup | null>(null);
  // Backup awaiting delete confirmation, or null. Drives the Modal-based confirm
  // (replaces window.confirm, which is unstyled and blocks the event loop).
  let deleteFor = $state<Backup | null>(null);

  async function reload() {
    loading = true;
    error = null;
    const r = await commands.listBackups(instanceId, worldFolder);
    loading = false;
    if (r.status === 'ok') backups = r.data;
    else error = formatError(r.error);
  }

  $effect(() => void reload());

  async function confirmDelete() {
    const b = deleteFor;
    if (!b) return;
    deleteFor = null;
    const r = await commands.deleteBackup(instanceId, worldFolder, b.filename);
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
    const r = await commands.backupWorld(instanceId, worldFolder);
    backingUp = false;
    if (r.status === 'ok') {
      onChanged();
      await reload();
    } else {
      error = formatError(r.error);
    }
  }

  async function onOpenBackupsFolder() {
    const r = await commands.openBackupsFolder(instanceId, worldFolder);
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

<!-- The folder button lives here, above the state branches, not inside the
     list branch. It used to render only when a list was shown, so it was
     missing while loading, on error, and — most importantly — when there are
     no backups yet, which is the case `open_backups_folder` was written for
     ("so the user can navigate even before the first backup exists"). -->
<div class="flex items-center justify-end gap-2 mb-3">
  <button
    type="button"
    class="btn-secondary btn-sm inline-flex items-center gap-1 flex-shrink-0"
    data-testid="backups-open-folder-btn"
    onclick={() => void onOpenBackupsFolder()}
  >
    <Icon name="folderOpen" size={14} />
    {$t('worlds.backups.openBackupsFolder')}
  </button>
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
            onclick={() => (deleteFor = b)}
          >
            <Icon name="trash" size={15} />
          </button>
        </div>
      </li>
    {/each}
  </ul>
  <div class="text-xs text-muted mb-3">
    <span>{$t('worlds.backups.total', { size: formatSize($t, totalSize) })}</span>
  </div>
{/if}

{#if restoreFor}
  <RestoreBackupDialog
    {instanceId}
    {worldFolder}
    {lockedMode}
    {modeNote}
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

{#if deleteFor}
  <!-- Modal-based delete confirm (replaces the unstyled, event-loop-blocking
       window.confirm). Same shape as DeleteServerDialog: secondary Cancel +
       danger Delete. Reuses the existing confirmDelete copy for the question.
       Rendered after the panel's own markup, and the panel itself is rendered
       inside WorldDetailDialog's Modal, so this confirm always mounts after
       its ancestor modal — the shared Modal stack gives Escape to this one. -->
  <Modal
    ariaLabelledby="backup-delete-confirm-title"
    onClose={() => (deleteFor = null)}
    panelClass="w-[420px] p-5 flex flex-col gap-3"
  >
    <h3 id="backup-delete-confirm-title" class="font-semibold text-primary text-base">
      {$t('worlds.backups.deleteBackup')}
    </h3>
    <p class="text-sm text-secondary">
      {$t('worlds.backups.confirmDelete', { timestamp: formatBackupTimestamp(deleteFor) })}
    </p>
    <div class="flex justify-end gap-2 mt-2">
      <button type="button" class="btn-secondary btn-sm" onclick={() => (deleteFor = null)}>
        {$t('common.cancel')}
      </button>
      <button type="button" class="btn-danger btn-sm" onclick={() => void confirmDelete()}>
        {$t('worlds.backups.deleteBackup')}
      </button>
    </div>
  </Modal>
{/if}
