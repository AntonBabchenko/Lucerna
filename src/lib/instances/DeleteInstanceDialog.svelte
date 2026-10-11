<script lang="ts">
  // Delete an instance — the Settings tab's Danger zone and the sidebar row
  // menu both open it, so it names its own target and is hosted by the page.
  //
  // Deleting removes ALL of the instance's worlds, mods and configs, so the gate
  // is the strongest the app has: the user types the literal word "Delete"
  // (DeleteWorldDialog's rule). It stays open until the result is in: a
  // multi-gigabyte removal must not look done before it is, and a failure is
  // shown here, where the user is looking (ImportedDetailDrawer's delete is the
  // precedent).
  import { commands, type InstanceWithStatus } from '$lib/ipc/bindings';
  import { instanceIpcErrorMessage } from '$lib/instances/instance-ipc-error';
  import { t } from '$lib/i18n';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';

  let {
    instance,
    refresh,
    onDeleted,
    onClose,
  }: {
    instance: InstanceWithStatus;
    /** The page's instance-list refresh. Runs after a failure too: the removal
     *  can stop part-way (a locked file), and what is left must show. */
    refresh: () => Promise<void>;
    onDeleted: () => void;
    onClose: () => void;
  } = $props();

  const DELETE_CONFIRM_WORD = 'Delete';
  let typed = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function confirm() {
    if (busy || typed !== DELETE_CONFIRM_WORD) return;
    busy = true;
    error = null;
    const id = instance.id;
    try {
      const result = await commands.deleteInstance(id);
      await refresh();
      if (result.status === 'ok') {
        onDeleted();
        onClose();
      } else {
        error = instanceIpcErrorMessage(result.error);
      }
    } finally {
      busy = false;
    }
  }
</script>

<ConfirmDialog
  title={$t('instance.delete.title')}
  confirmLabel={$t('instance.delete.confirmBtn')}
  variant="danger"
  {busy}
  confirmDisabled={typed !== DELETE_CONFIRM_WORD}
  confirmTestid="instance-delete-confirm"
  panelClass="w-[440px] max-w-full p-5 flex flex-col gap-3"
  onCancel={() => {
    if (!busy) onClose();
  }}
  onConfirm={() => void confirm()}
>
  {#snippet body()}
    <p class="text-sm text-secondary">{$t('instance.delete.question', { name: instance.name })}</p>
    <p class="text-sm text-secondary">{$t('instance.delete.description')}</p>
    <label class="block text-xs text-secondary" for="instance-delete-confirm-input">
      {$t('worlds.delete.typeToConfirm', { word: DELETE_CONFIRM_WORD })}
    </label>
    <input
      id="instance-delete-confirm-input"
      class="border rounded px-2 py-1 w-full"
      bind:value={typed}
      placeholder={DELETE_CONFIRM_WORD}
      autocomplete="off"
      disabled={busy}
      data-testid="instance-delete-confirm-input"
    />
    <StatusMessage tone="danger" message={error} dataTestid="instance-delete-error" />
  {/snippet}
</ConfirmDialog>
