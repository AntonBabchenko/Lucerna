<script lang="ts">
  import { commands, type Backup, type World } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import RestoreBackupDialog from '$lib/worlds/RestoreBackupDialog.svelte';
  import { t, locale } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import { Icon } from '$lib/ui/icons';
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
  let openMenuFor = $state<string | null>(null);
  let menuTop = $state(0);
  let menuLeft = $state(0);
  let restoreFor = $state<Backup | null>(null);

  // The backups list is a max-h-80 overflow-auto <ul> so the kebab's
  // absolute-positioned popover would be clipped inside that scrollable
  // box. Same fix as the sidebar (?)-tooltip in
  // InstanceConceptTooltip.svelte: render position:fixed with coords
  // measured from the trigger on open + close on scroll/resize.
  const MENU_WIDTH = 160; // = Tailwind w-40
  const GAP = 4;
  const MARGIN = 8;

  function toggleMenu(filename: string, e: MouseEvent) {
    if (openMenuFor === filename) {
      openMenuFor = null;
      return;
    }
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menuTop = r.bottom + GAP;
    const wantLeft = r.right - MENU_WIDTH; // right-align with trigger
    const maxLeft = window.innerWidth - MENU_WIDTH - MARGIN;
    menuLeft = Math.min(Math.max(wantLeft, MARGIN), Math.max(MARGIN, maxLeft));
    openMenuFor = filename;
  }

  $effect(() => {
    if (openMenuFor == null) return;
    const close = () => (openMenuFor = null);
    window.addEventListener('scroll', close, true);
    window.addEventListener('resize', close);
    return () => {
      window.removeEventListener('scroll', close, true);
      window.removeEventListener('resize', close);
    };
  });

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
    openMenuFor = null;
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

  async function onOpenBackupsFolder() {
    const r = await commands.openBackupsFolder(instanceId, world.folder_name);
    if (r.status !== 'ok') error = formatError(r.error);
  }

  function formatBytes(n: number | null | undefined): string {
    if (n == null) return '';
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
    return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
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
  <h3 id="backups-dialog-title" class="font-semibold text-lg text-primary mb-3">
    {$t('worlds.backups.title', { world: world.folder_name })}
  </h3>
  {#if loading}
    <p class="text-sm text-muted">{$t('worlds.backups.loading')}</p>
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
        <li>
          <button
            type="button"
            class="w-full flex items-center justify-between gap-2 px-3 py-2 text-left hover:bg-subtle"
            aria-label={$t('worlds.backups.backupActionsAriaLabel', { filename: b.filename })}
            aria-expanded={openMenuFor === b.filename}
            onclick={(e) => toggleMenu(b.filename, e)}
          >
            <div class="min-w-0">
              <div class="text-sm font-medium">{formatBackupTimestamp(b)}</div>
              <div class="text-xs text-muted">{formatBytes(b.size_bytes)}</div>
            </div>
            <span class="text-placeholder flex-shrink-0" aria-hidden="true"
              ><Icon name="moreVertical" size={16} /></span
            >
          </button>
        </li>
      {/each}
    </ul>
    <div class="text-xs text-muted mb-3 flex justify-between">
      <span>{$t('worlds.backups.total', { size: formatBytes(totalSize) })}</span>
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

{#if openMenuFor}
  {@const activeBackup = backups.find((b) => b.filename === openMenuFor)}
  {#if activeBackup}
    <!-- Click-outside backdrop. z-[60] sits above the dialog (z-50). -->
    <button
      type="button"
      class="fixed inset-0 z-[60]"
      aria-label={$t('worlds.backups.closeMenuAriaLabel')}
      onclick={() => (openMenuFor = null)}
    ></button>
    <div
      class="fixed z-[70] w-40 bg-surface border border-border-subtle rounded shadow"
      style="top: {menuTop}px; left: {menuLeft}px;"
      role="menu"
    >
      <button
        type="button"
        role="menuitem"
        class="block w-full text-left px-3 py-2 text-sm hover:bg-subtle"
        onclick={() => {
          restoreFor = activeBackup;
          openMenuFor = null;
        }}
      >
        {$t('worlds.backups.restore')}
      </button>
      <button
        type="button"
        role="menuitem"
        class="block w-full text-left px-3 py-2 text-sm hover:bg-subtle text-danger"
        onclick={() => void onDelete(activeBackup)}
      >
        {$t('worlds.backups.deleteBackup')}
      </button>
    </div>
  {/if}
{/if}

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
