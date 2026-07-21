<script lang="ts">
  import { commands, type World } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';

  let {
    instanceId,
    world,
    onClose,
    onDeleted,
  }: {
    instanceId: string;
    world: World;
    onClose: () => void;
    onDeleted: () => void;
  } = $props();

  // Single-confirm danger dialog — no type-to-confirm gate, matching
  // DeleteServerDialog. titleSize="lg" preserves this dialog's original heading
  // (it predates the text-base forward rule — new dialogs should omit it).
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function onConfirm() {
    busy = true;
    error = null;
    const r = await commands.deleteWorld(instanceId, world.folder_name);
    busy = false;
    if (r.status === 'ok') {
      onDeleted();
    } else {
      error = formatError(r.error);
    }
  }
</script>

<ConfirmDialog
  title={$t('worlds.delete.title', { world: world.folder_name })}
  bodyText={$t('worlds.delete.description')}
  titleSize="lg"
  variant="danger"
  confirmLabel={$t('worlds.delete.deleteBtn')}
  {busy}
  {error}
  panelClass="max-w-md w-full p-4 flex flex-col gap-3"
  onCancel={onClose}
  onConfirm={() => void onConfirm()}
/>
