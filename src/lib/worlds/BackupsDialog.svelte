<script lang="ts">
  import { commands, type Backup, type World } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import RestoreBackupDialog from '$lib/worlds/RestoreBackupDialog.svelte';

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
    openMenuFor = null;
    if (!confirm(`Delete backup ${formatBackupTimestamp(b)}?`)) return;
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
    return new Date(b.created_unix_ms).toLocaleString('en-GB', {
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

<div
  class="fixed inset-0 z-50 bg-black/30 flex items-center justify-center"
  role="dialog"
  aria-modal="true"
  aria-labelledby="backups-dialog-title"
>
  <div class="bg-white border border-neutral-200 rounded shadow-lg max-w-lg w-full p-4">
    <h3 id="backups-dialog-title" class="font-semibold text-lg mb-3">
      Backups for "{world.folder_name}"
    </h3>
    {#if loading}
      <p class="text-sm text-neutral-500">Loading backups…</p>
    {:else if error}
      <p class="text-sm text-red-700 mb-2">{error}</p>
    {:else if backups.length === 0}
      <p class="text-sm text-neutral-500">
        No backups yet. Click "Back up now" on the world to create one.
      </p>
    {:else}
      <ul
        class="border border-neutral-200 rounded divide-y divide-neutral-200 mb-3 max-h-80 overflow-auto"
      >
        {#each backups as b (b.filename)}
          <li class="flex items-center justify-between gap-2 px-3 py-2">
            <div class="min-w-0">
              <div class="text-sm font-medium">{formatBackupTimestamp(b)}</div>
              <div class="text-xs text-neutral-500">{formatBytes(b.size_bytes)}</div>
            </div>
            <div class="relative">
              <button
                type="button"
                class="border rounded px-2 py-1 text-sm hover:bg-neutral-50"
                aria-label="Actions for backup {b.filename}"
                onclick={() => (openMenuFor = openMenuFor === b.filename ? null : b.filename)}
              >
                ⋮
              </button>
              {#if openMenuFor === b.filename}
                <div
                  class="absolute right-0 mt-1 w-40 bg-white border border-neutral-200 rounded shadow z-10"
                  role="menu"
                >
                  <button
                    type="button"
                    role="menuitem"
                    class="block w-full text-left px-3 py-2 text-sm hover:bg-neutral-50"
                    onclick={() => {
                      openMenuFor = null;
                      restoreFor = b;
                    }}
                  >
                    Restore…
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    class="block w-full text-left px-3 py-2 text-sm hover:bg-neutral-50 text-red-700"
                    onclick={() => void onDelete(b)}
                  >
                    Delete backup
                  </button>
                </div>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
      <div class="text-xs text-neutral-500 mb-3 flex justify-between">
        <span>Total: {formatBytes(totalSize)}</span>
        <button
          type="button"
          class="text-blue-700 hover:underline"
          onclick={() => void onOpenBackupsFolder()}
        >
          Open backups folder ↗
        </button>
      </div>
    {/if}
    <div class="flex justify-end">
      <button type="button" class="border rounded px-3 py-1 text-sm" onclick={onClose}>
        Close
      </button>
    </div>
  </div>
</div>

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
