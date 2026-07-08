<script lang="ts">
  // Shown when the user tries to hide the account-add buttons while no account
  // exists yet. Hiding them then would be a dead end — you can't sign in and
  // can't launch the game — so instead of the hide-confirm we explain that an
  // account is required first. Thin info-modal wrapper over Modal (single
  // acknowledge button); compact-friendly width so it fits the mini-mode strip.
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';

  let { onClose }: { onClose: () => void } = $props();
</script>

<Modal
  ariaLabelledby="account-required-title"
  {onClose}
  panelClass="w-[min(360px,92vw)] p-5 flex flex-col gap-3"
>
  <h3 id="account-required-title" class="font-semibold text-primary text-base">
    {$t('sidebar.accountRequired.title')}
  </h3>
  <p class="text-sm text-secondary">
    {$t('sidebar.accountRequired.body')}
  </p>
  <div class="flex justify-end mt-2">
    <button
      type="button"
      class="btn-primary btn-sm"
      data-testid="account-required-ok"
      onclick={onClose}
    >
      {$t('sidebar.accountRequired.ok')}
    </button>
  </div>
</Modal>
