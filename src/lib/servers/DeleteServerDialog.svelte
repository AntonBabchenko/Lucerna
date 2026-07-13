<script lang="ts">
  // Confirmation gate for deleting a server. Deletion is irreversible (world,
  // mods, config are removed), so a single click must not delete silently.
  // Mirrors RemoveAccountDialog. Purely presentational — the serverState.remove
  // call lives in the caller (ServerSettingsTab).
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
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

<ConfirmDialog
  title={$t('servers.delete.title')}
  bodyText={[$t('servers.delete.question', { name: serverName }), $t('servers.delete.description')]}
  variant="danger"
  confirmLabel={$t('servers.delete.confirm')}
  {onCancel}
  {onConfirm}
/>
