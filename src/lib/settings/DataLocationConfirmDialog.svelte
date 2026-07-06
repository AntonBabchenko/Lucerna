<script lang="ts">
  // Confirmation gate before relocating (or resetting) the data root.
  // `setDataLocation` restarts the app once it finishes, so the user must
  // knowingly accept that before it fires. Mirrors DeleteServerDialog's
  // shape (thin presentational wrapper over Modal; the mutation stays with
  // the caller — StoragePanel).
  import Modal from '$lib/ui/Modal.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { t } from '$lib/i18n';

  let {
    targetPath,
    sizeLabel,
    busy,
    onCancel,
    onConfirm,
  }: {
    /** null = resetting to the default location. */
    targetPath: string | null;
    /** Pre-formatted human-readable size ("1.2 GB"), already localized. */
    sizeLabel: string;
    busy: boolean;
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();
</script>

<Modal
  ariaLabelledby="data-location-confirm-title"
  onClose={onCancel}
  closeOnBackdrop={!busy}
  closeOnEscape={!busy}
  panelClass="w-[480px] p-5 flex flex-col gap-3"
>
  <h3 id="data-location-confirm-title" class="font-semibold text-primary text-base">
    {targetPath
      ? $t('settings.storage.dataLocation.confirm.moveTitle')
      : $t('settings.storage.dataLocation.confirm.resetTitle')}
  </h3>
  <p class="text-sm text-secondary">
    {targetPath
      ? $t('settings.storage.dataLocation.confirm.moveBody', { path: targetPath, size: sizeLabel })
      : $t('settings.storage.dataLocation.confirm.resetBody', { size: sizeLabel })}
  </p>
  <p class="text-sm text-secondary font-medium">
    {$t('settings.storage.dataLocation.confirm.restartNote')}
  </p>
  <div class="flex justify-end gap-2 mt-2">
    <button type="button" class="btn-secondary btn-sm" disabled={busy} onclick={onCancel}>
      {$t('common.cancel')}
    </button>
    <BusyButton class="btn-primary btn-sm" {busy} onclick={onConfirm}>
      {$t('settings.storage.dataLocation.confirm.confirmBtn')}
    </BusyButton>
  </div>
</Modal>
