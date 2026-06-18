<script lang="ts">
  import { commands, type Backup, type RestoreMode } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import { t, locale } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';

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
      pushSuccess(
        $t('worlds.restore.toastRestored', {
          world: worldFolder,
          result: r.data.final_folder_name,
        }),
      );
      onRestored();
    } else {
      error = formatError(r.error);
    }
  }

  function formatTs(): string {
    if (!backup.created_unix_ms) return backup.filename;
    return new Date(backup.created_unix_ms).toLocaleString($locale ?? undefined);
  }
</script>

<Modal
  ariaLabelledby="restore-dialog-title"
  {onClose}
  panelClass="max-w-md w-full p-4"
  closeOnBackdrop={!busy}
  closeOnEscape={!busy}
>
  <h3 id="restore-dialog-title" class="font-semibold text-lg text-primary mb-3">
    {$t('worlds.restore.title', { world: worldFolder, timestamp: formatTs() })}
  </h3>
  <label class="flex items-start gap-2 mb-3">
    <input type="radio" name="restore-mode" value="replace" bind:group={mode} disabled={busy} />
    <span class="text-sm">
      <span class="font-medium">{$t('worlds.restore.replaceLabel')}</span>
      <br />
      <span class="text-secondary">
        {$t('worlds.restore.replaceDescription')}
      </span>
    </span>
  </label>
  <label class="flex items-start gap-2 mb-4">
    <input type="radio" name="restore-mode" value="as_copy" bind:group={mode} disabled={busy} />
    <span class="text-sm">
      <span class="font-medium">{$t('worlds.restore.asCopyLabel')}</span>
      <br />
      <span class="text-secondary">
        {$t('worlds.restore.asCopyDescription', { world: worldFolder })}
      </span>
    </span>
  </label>
  {#if error}
    <p class="text-xs text-danger mb-2">{error}</p>
  {/if}
  <div class="flex justify-end gap-2">
    <button type="button" class="btn-secondary btn-sm" onclick={onClose} disabled={busy}>
      {$t('common.cancel')}
    </button>
    <BusyButton type="button" class="btn-primary btn-sm" onclick={() => void onConfirm()} {busy}>
      {$t('worlds.restore.restoreBtn')}
    </BusyButton>
  </div>
</Modal>
