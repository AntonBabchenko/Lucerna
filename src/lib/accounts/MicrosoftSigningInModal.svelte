<script lang="ts">
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';

  let {
    open = false,
    onCancel,
  }: {
    open?: boolean;
    onCancel?: () => void;
  } = $props();
</script>

{#if open}
  <!-- Shared Modal shell carries role="dialog" + aria-modal, the focus trap,
       initial focus, focus restore and Escape → onCancel, so this waiting
       dialog no longer hand-rolls (and mis-implements) any of that. -->
  <Modal
    ariaLabelledby="ms-signing-in-heading"
    onClose={() => onCancel?.()}
    panelClass="max-w-md w-full p-6"
  >
    <h3 id="ms-signing-in-heading" class="text-lg font-semibold text-primary">
      {$t('accounts.signingIn.heading')}
    </h3>
    <div class="mt-3 flex items-start gap-3 text-sm text-secondary">
      <Spinner size="sm" class="mt-0.5 shrink-0 text-accent" />
      <p>{$t('accounts.signingIn.body')}</p>
    </div>
    <div class="mt-6 flex justify-end gap-2">
      <button type="button" class="btn-secondary btn-sm" onclick={() => onCancel?.()}>
        {$t('common.cancel')}
      </button>
    </div>
  </Modal>
{/if}
