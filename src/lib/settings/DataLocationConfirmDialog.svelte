<script lang="ts">
  // Confirmation gate before moving, resetting or adopting the data root. Thin presentational
  // wrapper over ConfirmDialog (the mutation stays with the caller — StoragePanel). A clean move
  // or adopt restarts the app, so the user must knowingly accept that here; a move can also end
  // cancelled or with a restart pending, which the app-level DataMoveHost reports.
  //
  // Sizes arrive as NUMBERS from the backend plan and are formatted here, so an unknown size is a
  // missing row — never "0 B". Free space only ever warns (spec §2): the estimate under-counts
  // unreadable entries, cluster overhead varies, and a failed copy is cleaned up, so a wrong
  // guess costs time, not data.
  import { formatSize } from '$lib/format/size';
  import { t } from '$lib/i18n';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';

  /** Warn when free < required + max(required / 50, 64 MiB) — see the IPC contract. */
  const LOW_SPACE_MARGIN_DIVISOR = 50;
  const LOW_SPACE_MIN_MARGIN_BYTES = 64 * 1024 * 1024;

  let {
    mode,
    fromPath,
    toPath,
    detachedPath = null,
    pointerOnly = false,
    recoverySession = false,
    requiredBytes = null,
    freeBytes = null,
    currentSizeBytes = null,
    busy,
    onCancel,
    onConfirm,
  }: {
    /** move = copy into a fresh target; reset = move back to the default location; adopt =
     * repoint at an existing root without copying. */
    mode: 'move' | 'reset' | 'adopt';
    /** The current effective root. */
    fromPath: string;
    /** move / adopt: the backend-planned target; reset: the default folder. */
    toPath: string;
    /** Pointer-only reset: the configured, unavailable folder being detached; null when the
     * pointer could not be read — there is no folder to name. */
    detachedPath?: string | null;
    /** Reset while fallen back: nothing is copied, only the redirect goes. `toPath` is then the
     * folder the launcher will START FROM, as predicted by the real resolver. */
    pointerOnly?: boolean;
    /** The launcher runs on a throwaway session root: it is never "your current data". */
    recoverySession?: boolean;
    /** The plan's estimate of what will be copied; null = unknown. */
    requiredBytes?: number | null;
    /** Free space on the target volume; null = could not check. */
    freeBytes?: number | null;
    /** Adopt only: size of the data that stays behind; null = unknown. */
    currentSizeBytes?: number | null;
    busy: boolean;
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();

  const sizeText = $derived(formatSize($t, requiredBytes));
  const currentSizeText = $derived(formatSize($t, currentSizeBytes));
  // A known zero is an answer ("0 B free"); only null is "could not check".
  const freeText = $derived(
    freeBytes === null ? '' : formatSize($t, freeBytes) || $t('format.size.bytes', { n: 0 }),
  );
  // `requiredBytes > 0`: an empty root has nothing to copy, and the sentence needs a size to name.
  const lowSpace = $derived(
    requiredBytes !== null &&
      requiredBytes > 0 &&
      freeBytes !== null &&
      freeBytes <
        requiredBytes +
          Math.max(requiredBytes / LOW_SPACE_MARGIN_DIVISOR, LOW_SPACE_MIN_MARGIN_BYTES),
  );
</script>

{#snippet row(label: string, value: string, testid: string, mono: boolean)}
  <div class="flex gap-2">
    <dt class="shrink-0 text-muted">{label}</dt>
    <dd
      class={mono ? 'font-mono text-xs selectable break-all' : 'text-secondary'}
      data-testid={testid}
    >
      {value}
    </dd>
  </div>
{/snippet}

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
  panelClass="w-[480px] max-w-full p-5 flex flex-col gap-3"
  {busy}
  {onCancel}
  {onConfirm}
>
  {#snippet body()}
    {#if mode === 'adopt'}
      <p class="text-sm text-secondary">
        {$t('settings.storage.dataLocation.confirm.adoptBody', { path: toPath })}
      </p>
      {#if !recoverySession}
        <p class="text-sm text-secondary font-medium">
          {currentSizeText
            ? $t('settings.storage.dataLocation.confirm.adoptCurrentNote', {
                current: fromPath,
                size: currentSizeText,
              })
            : $t('settings.storage.dataLocation.confirm.adoptCurrentNoteNoSize', {
                current: fromPath,
              })}
        </p>
      {/if}
      <p class="text-sm text-secondary font-medium">
        {$t('settings.storage.dataLocation.confirm.adoptRestartNote')}
      </p>
    {:else if pointerOnly}
      <p class="text-sm text-secondary">
        {detachedPath
          ? $t('settings.storage.dataLocation.confirm.resetPointerOnlyBody', {
              path: detachedPath,
              landing: toPath,
            })
          : $t('settings.storage.dataLocation.confirm.resetPointerOnlyBodyNoPath', {
              landing: toPath,
            })}
      </p>
      <p class="text-sm text-secondary font-medium">
        {$t('settings.storage.dataLocation.confirm.resetPointerOnlyRestartNote')}
      </p>
    {:else}
      <p class="text-sm text-secondary">
        {mode === 'move'
          ? $t('settings.storage.dataLocation.confirm.moveIntro')
          : $t('settings.storage.dataLocation.confirm.resetIntro')}
      </p>
      <dl class="flex flex-col gap-1 text-sm">
        {@render row(
          $t('settings.storage.dataLocation.confirm.fromLabel'),
          fromPath,
          'data-move-from',
          true,
        )}
        {@render row(
          $t('settings.storage.dataLocation.confirm.toLabel'),
          toPath,
          'data-move-to',
          true,
        )}
        {#if sizeText}
          {@render row(
            $t('settings.storage.dataLocation.confirm.copySizeLabel'),
            $t('settings.storage.dataLocation.confirm.copySizeValue', { size: sizeText }),
            'data-move-size',
            false,
          )}
        {/if}
        {#if freeText}
          {@render row(
            $t('settings.storage.dataLocation.confirm.freeLabel'),
            freeText,
            'data-move-free',
            false,
          )}
        {/if}
      </dl>
      {#if lowSpace}
        <div data-testid="data-move-low-space">
          <StatusMessage
            message={$t('settings.storage.dataLocation.confirm.lowSpace', {
              required: sizeText,
              free: freeText,
            })}
            tone="warning"
            withIcon
          />
        </div>
      {/if}
      {#if freeBytes === null}
        <div data-testid="data-move-free-unknown">
          <StatusMessage
            message={$t('settings.storage.dataLocation.confirm.freeUnknown')}
            tone="warning"
            withIcon
          />
        </div>
      {/if}
      <p class="text-sm text-secondary font-medium">
        {$t('settings.storage.dataLocation.confirm.cancelNote')}
      </p>
      {#if mode === 'move'}
        <p class="text-xs text-muted">
          {$t('settings.storage.dataLocation.confirm.lucernaDataNote')}
        </p>
      {/if}
    {/if}
  {/snippet}
</ConfirmDialog>
