<script lang="ts">
  // Confirmation gate for deleting a server. Deletion is irreversible (world,
  // mods, config are removed), so a single click must not delete silently.
  // Mirrors RemoveAccountDialog. Purely presentational — the serverState.remove
  // call lives in the caller (ServersView).
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';

  let {
    serverName,
    onCancel,
    onConfirm,
  }: {
    serverName: string;
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();
</script>

<Modal
  ariaLabelledby="server-delete-confirm-title"
  onClose={onCancel}
  panelClass="w-[440px] p-5 flex flex-col gap-3"
>
  <h3 id="server-delete-confirm-title" class="font-semibold text-primary text-base">
    {$t('servers.delete.title')}
  </h3>
  <p class="text-sm text-secondary">
    {$t('servers.delete.question', { name: serverName })}
  </p>
  <p class="text-sm text-secondary">
    {$t('servers.delete.description')}
  </p>
  <div class="flex justify-end gap-2 mt-2">
    <button type="button" class="btn-secondary btn-sm" onclick={onCancel}>
      {$t('common.cancel')}
    </button>
    <button type="button" class="btn-danger btn-sm" onclick={onConfirm}>
      {$t('servers.delete.confirm')}
    </button>
  </div>
</Modal>
