<script lang="ts">
  /*
    Shared confirm-dialog primitive. Collapses the near-identical
    presentational wrappers over <Modal> (RemoveAccountDialog,
    HideButtonConfirmDialog, DataLocationConfirmDialog, DeleteWorldDialog,
    DeleteServerDialog) into one shape.

    Presentational only — the mutation stays with the caller via `onConfirm`,
    exactly like the dialogs it replaces. All copy is passed in ALREADY
    LOCALIZED, so this file imports no message keys except the common Cancel
    fallback. This keeps i18n ownership with the caller and matches the
    existing "thin wrapper over Modal, mutation stays with the caller" note.

    Closes two DESIGN.md "Known gaps":
      • Dialog-title size drift — renders <DialogTitle> (defaults to text-base,
        the forward rule; pass titleSize="lg" only for wizard/import flows).
      • Modal has no aria-describedby — wires the body's id through Modal's new
        `ariaDescribedby` prop so a screen reader announces the body copy, not
        just the title.
  */
  import type { Snippet } from 'svelte';
  import Modal from './Modal.svelte';
  import BusyButton from './BusyButton.svelte';
  import DialogTitle from './DialogTitle.svelte';
  import { t } from '$lib/i18n';

  let {
    title,
    body,
    bodyText,
    confirmLabel,
    cancelLabel,
    variant = 'primary',
    titleSize = 'base',
    busy = false,
    error = null,
    confirmTestid,
    panelClass = 'w-[440px] p-5 flex flex-col gap-3',
    onCancel,
    onConfirm,
  }: {
    /** Already-localized title. */
    title: string;
    /** Rich body content; when set it replaces `bodyText`. */
    body?: Snippet;
    /** Plain body copy — a string, or an array rendered as stacked paragraphs. */
    bodyText?: string | string[];
    /** Already-localized confirm label; for destructive dialogs this IS the verb. */
    confirmLabel: string;
    /** Defaults to the shared common.cancel string. */
    cancelLabel?: string;
    /** 'danger' → .btn-danger confirm; 'primary' → .btn-primary confirm. */
    variant?: 'primary' | 'danger';
    /** 'base' (default forward rule) or 'lg' (wizard/import flows only). */
    titleSize?: 'base' | 'lg';
    busy?: boolean;
    error?: string | null;
    confirmTestid?: string;
    panelClass?: string;
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();

  const uid = crypto.randomUUID();
  const titleId = `confirm-title-${uid}`;
  const bodyId = `confirm-body-${uid}`;

  const paragraphs = $derived(
    bodyText == null ? [] : Array.isArray(bodyText) ? bodyText : [bodyText],
  );

  const confirmClass = $derived(`${variant === 'danger' ? 'btn-danger' : 'btn-primary'} btn-sm`);
  const resolvedCancel = $derived(cancelLabel ?? $t('common.cancel'));
</script>

<Modal
  ariaLabelledby={titleId}
  ariaDescribedby={bodyId}
  onClose={onCancel}
  closeOnBackdrop={!busy}
  closeOnEscape={!busy}
  {panelClass}
>
  <DialogTitle id={titleId} size={titleSize}>{title}</DialogTitle>

  <div id={bodyId} class="flex flex-col gap-2">
    {#if body}
      {@render body()}
    {:else}
      {#each paragraphs as p}
        <p class="text-sm text-secondary">{p}</p>
      {/each}
    {/if}
  </div>

  {#if error}
    <p class="text-xs text-danger">{error}</p>
  {/if}

  <div class="flex justify-end gap-2 mt-2">
    <button type="button" class="btn-secondary btn-sm" disabled={busy} onclick={onCancel}>
      {resolvedCancel}
    </button>
    <BusyButton
      {busy}
      type="button"
      class={confirmClass}
      data-testid={confirmTestid}
      onclick={onConfirm}
    >
      {confirmLabel}
    </BusyButton>
  </div>
</Modal>
