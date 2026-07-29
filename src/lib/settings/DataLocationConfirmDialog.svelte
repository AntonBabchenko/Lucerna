<script lang="ts">
  // Confirmation gate before relocating (or resetting/adopting) the data
  // root. Both backend commands restart the app on success, so the user must
  // knowingly accept that before either fires. Thin presentational wrapper
  // over ConfirmDialog (the mutation stays with the caller — StoragePanel).
  // Uses the body snippet so the emphasized notes keep their font-medium
  // weight, which a plain bodyText paragraph would flatten.
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import { t } from '$lib/i18n';

  let {
    mode,
    targetPath,
    currentPath,
    sizeLabel,
    busy,
    pointerOnly = false,
    onCancel,
    onConfirm,
  }: {
    /** move = copy into a fresh target; reset = move back to the default
     * location; adopt = repoint at an existing root without copying. */
    mode: 'move' | 'reset' | 'adopt';
    /** Effective target path. For reset: the configured (possibly
     * unavailable) folder — only the pointer-only body shows it. */
    targetPath: string;
    /** Current effective root; shown in adopt's stays-on-disk note. */
    currentPath: string;
    /** Pre-formatted human-readable size ("1.2 GB"), already localized. */
    sizeLabel: string;
    busy: boolean;
    /** Reset while fallen back: the configured folder is unavailable, so no
     * data is copied — only the redirect is removed. The body must not
     * promise a move that will not happen. */
    pointerOnly?: boolean;
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();
</script>

<ConfirmDialog
  title={mode === 'adopt'
    ? $t('settings.storage.dataLocation.confirm.adoptTitle')
    : mode === 'move'
      ? $t('settings.storage.dataLocation.confirm.moveTitle')
      : $t('settings.storage.dataLocation.confirm.resetTitle')}
  confirmLabel={mode === 'adopt'
    ? $t('settings.storage.dataLocation.confirm.adoptConfirmBtn')
    : mode === 'reset' && pointerOnly
      ? $t('settings.storage.dataLocation.confirm.resetPointerOnlyConfirmBtn')
      : $t('settings.storage.dataLocation.confirm.confirmBtn')}
  panelClass="w-[480px] p-5 flex flex-col gap-3"
  {busy}
  {onCancel}
  {onConfirm}
>
  {#snippet body()}
    {#if mode === 'adopt'}
      <p class="text-sm text-secondary">
        {$t('settings.storage.dataLocation.confirm.adoptBody', { path: targetPath })}
      </p>
      <p class="text-sm text-secondary font-medium">
        {$t('settings.storage.dataLocation.confirm.adoptCurrentNote', {
          current: currentPath,
          size: sizeLabel,
        })}
      </p>
    {:else}
      <p class="text-sm text-secondary">
        {mode === 'move'
          ? $t('settings.storage.dataLocation.confirm.moveBody', {
              path: targetPath,
              size: sizeLabel,
            })
          : pointerOnly
            ? $t('settings.storage.dataLocation.confirm.resetPointerOnlyBody', {
                path: targetPath,
              })
            : $t('settings.storage.dataLocation.confirm.resetBody', { size: sizeLabel })}
      </p>
    {/if}
    <p class="text-sm text-secondary font-medium">
      {mode === 'adopt'
        ? $t('settings.storage.dataLocation.confirm.adoptRestartNote')
        : $t('settings.storage.dataLocation.confirm.restartNote')}
    </p>
  {/snippet}
</ConfirmDialog>
