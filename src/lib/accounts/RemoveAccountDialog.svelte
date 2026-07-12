<script lang="ts">
  // Confirmation gate for removing an account. Account removal is recoverable
  // but costly — a Microsoft account needs a full browser OAuth round-trip to
  // re-add and its keychain tokens are purged; an offline account must be
  // re-created by name — so a single click should not delete it silently. This
  // mirrors the instance-delete confirm pattern (ManageInstancesModal). Purely
  // presentational: the actual removeAccount call lives in the page handler.
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import { t } from '$lib/i18n';

  let {
    accountName,
    onCancel,
    onConfirm,
  }: {
    accountName: string;
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();
</script>

<ConfirmDialog
  title={$t('page.accounts.removeTitle')}
  bodyText={[
    $t('page.accounts.removeQuestion', { name: accountName }),
    $t('page.accounts.removeDescription'),
  ]}
  variant="danger"
  confirmLabel={$t('page.accounts.removeConfirm')}
  {onCancel}
  {onConfirm}
/>
