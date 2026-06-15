<script lang="ts">
  // Confirmation gate for removing an account. Account removal is recoverable
  // but costly — a Microsoft account needs a full browser OAuth round-trip to
  // re-add and its keychain tokens are purged; an offline account must be
  // re-created by name — so a single click should not delete it silently. This
  // mirrors the instance-delete confirm pattern (ManageInstancesModal). Purely
  // presentational: the actual removeAccount call lives in the page handler.
  import Modal from '$lib/ui/Modal.svelte';
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

<Modal
  ariaLabelledby="account-remove-confirm-title"
  onClose={onCancel}
  panelClass="w-[440px] p-5 flex flex-col gap-3"
>
  <h3 id="account-remove-confirm-title" class="font-semibold text-primary text-base">
    {$t('page.accounts.removeTitle')}
  </h3>
  <p class="text-sm text-secondary">
    {$t('page.accounts.removeQuestion', { name: accountName })}
  </p>
  <p class="text-sm text-secondary">
    {$t('page.accounts.removeDescription')}
  </p>
  <div class="flex justify-end gap-2 mt-2">
    <button type="button" class="btn-secondary btn-sm" onclick={onCancel}>
      {$t('common.cancel')}
    </button>
    <button type="button" class="btn-danger btn-sm" onclick={onConfirm}>
      {$t('page.accounts.removeConfirm')}
    </button>
  </div>
</Modal>
