<script lang="ts">
  // Confirm gate for hiding a sidebar button via its right-click menu. Thin
  // presentational wrapper over ConfirmDialog (primary CTA — hiding is
  // reversible from Settings); the setHidden mutation stays with the caller
  // (Sidebar). Panel width is compact-friendly so the dialog fits the mini-mode
  // sidebar strip, where a toast would render off-window.
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
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

<ConfirmDialog
  title={$t('sidebar.hideConfirm.title')}
  bodyText={$t('sidebar.hideConfirm.body', { label })}
  confirmLabel={$t('sidebar.hideConfirm.confirm')}
  confirmTestid="hide-button-confirm"
  panelClass="w-[min(360px,92vw)] p-5 flex flex-col gap-3"
  {onCancel}
  {onConfirm}
/>
