<script lang="ts">
  // Confirmation gate before relocating (or resetting) the data root.
  // `setDataLocation` restarts the app once it finishes, so the user must
  // knowingly accept that before it fires. Thin presentational wrapper over
  // ConfirmDialog (the mutation stays with the caller — StoragePanel). Uses the
  // body snippet so the restart note keeps its font-medium emphasis, which a
  // plain bodyText paragraph would flatten.
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
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

<ConfirmDialog
  title={targetPath
    ? $t('settings.storage.dataLocation.confirm.moveTitle')
    : $t('settings.storage.dataLocation.confirm.resetTitle')}
  confirmLabel={$t('settings.storage.dataLocation.confirm.confirmBtn')}
  panelClass="w-[480px] p-5 flex flex-col gap-3"
  {busy}
  {onCancel}
  {onConfirm}
>
  {#snippet body()}
    <p class="text-sm text-secondary">
      {targetPath
        ? $t('settings.storage.dataLocation.confirm.moveBody', {
            path: targetPath,
            size: sizeLabel,
          })
        : $t('settings.storage.dataLocation.confirm.resetBody', { size: sizeLabel })}
    </p>
    <p class="text-sm text-secondary font-medium">
      {$t('settings.storage.dataLocation.confirm.restartNote')}
    </p>
  {/snippet}
</ConfirmDialog>
