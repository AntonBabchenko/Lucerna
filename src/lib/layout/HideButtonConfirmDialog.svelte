<script lang="ts">
  // Confirm gate for hiding a sidebar button via its right-click menu. Thin
  // presentational wrapper over Modal (mirrors DataLocationConfirmDialog); the
  // setHidden mutation stays with the caller (Sidebar). Panel width is
  // compact-friendly so the dialog fits the mini-mode sidebar strip, where a
  // toast would render off-window.
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';

  let {
    label,
    onCancel,
    onConfirm,
  }: {
    /** Human-readable name of the button being hidden (already localized). */
    label: string;
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();
</script>

<Modal
  ariaLabelledby="hide-button-confirm-title"
  onClose={onCancel}
  panelClass="w-[min(360px,92vw)] p-5 flex flex-col gap-3"
>
  <h3 id="hide-button-confirm-title" class="font-semibold text-primary text-base">
    {$t('sidebar.hideConfirm.title')}
  </h3>
  <p class="text-sm text-secondary">
    {$t('sidebar.hideConfirm.body', { label })}
  </p>
  <div class="flex justify-end gap-2 mt-2">
    <button type="button" class="btn-secondary btn-sm" onclick={onCancel}>
      {$t('common.cancel')}
    </button>
    <button
      type="button"
      class="btn-primary btn-sm"
      data-testid="hide-button-confirm"
      onclick={onConfirm}
    >
      {$t('sidebar.hideConfirm.confirm')}
    </button>
  </div>
</Modal>
