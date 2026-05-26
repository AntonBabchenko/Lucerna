<script lang="ts">
  import { commands, type Backup, type RestoreMode } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';

  let {
    instanceId,
    worldFolder,
    backup,
    onClose,
    onRestored,
  }: {
    instanceId: string;
    worldFolder: string;
    backup: Backup;
    onClose: () => void;
    onRestored: () => void;
  } = $props();

  let mode = $state<RestoreMode>('replace');
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function onConfirm() {
    busy = true;
    error = null;
    const r = await commands.restoreBackup(instanceId, worldFolder, backup.filename, mode);
    busy = false;
    if (r.status === 'ok') {
      pushSuccess(`Restored ${worldFolder} → ${r.data.final_folder_name}`);
      onRestored();
    } else {
      error = formatError(r.error);
    }
  }

  function formatTs(): string {
    if (!backup.created_unix_ms) return backup.filename;
    return new Date(backup.created_unix_ms).toLocaleString('en-GB');
  }
</script>

<div
  class="fixed inset-0 z-50 bg-black/30 flex items-center justify-center"
  role="dialog"
  aria-modal="true"
  aria-labelledby="restore-dialog-title"
>
  <div class="bg-surface border border-border-subtle rounded shadow-lg max-w-md w-full p-4">
    <h3 id="restore-dialog-title" class="font-semibold text-lg mb-3">
      Restore "{worldFolder}" from {formatTs()}
    </h3>
    <label class="flex items-start gap-2 mb-3">
      <input type="radio" name="restore-mode" value="replace" bind:group={mode} disabled={busy} />
      <span class="text-sm">
        <span class="font-medium">Replace current world</span>
        <br />
        <span class="text-secondary">
          Current state will be auto-saved as a backup first (labeled "pre-restore-…") so you can
          roll back if needed.
        </span>
      </span>
    </label>
    <label class="flex items-start gap-2 mb-4">
      <input type="radio" name="restore-mode" value="as_copy" bind:group={mode} disabled={busy} />
      <span class="text-sm">
        <span class="font-medium">Restore as a copy</span>
        <br />
        <span class="text-secondary">
          Original world stays untouched. The backup is unzipped as "{worldFolder} (restored)".
        </span>
      </span>
    </label>
    {#if error}
      <p class="text-xs text-danger mb-2">{error}</p>
    {/if}
    <div class="flex justify-end gap-2">
      <button
        type="button"
        class="border rounded px-3 py-1 text-sm"
        onclick={onClose}
        disabled={busy}
      >
        Cancel
      </button>
      <button
        type="button"
        class="bg-accent text-white rounded px-3 py-1 text-sm hover:bg-accent disabled:bg-muted"
        onclick={() => void onConfirm()}
        disabled={busy}
      >
        Restore
      </button>
    </div>
  </div>
</div>
