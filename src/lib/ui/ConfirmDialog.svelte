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

    Optional secondary action (2026-09-28, the guarded mod operations): an alternative between
    Cancel and the confirm («Только этот» next to «Отключить все N»). Always .btn-secondary — it is
    never the headline. Both actions mutate asynchronously, so each has its own busy flag: while
    either runs, backdrop/Escape are locked and every button is disabled, and only the running one
    spins. Rendered only when the caller passes both `secondaryLabel` and `onSecondary`, so every
    existing caller keeps its two buttons.

    `confirmDisabled` turns the confirm off without a spinner — a confirm over a subset the user
    picks (the update review) has nothing to run while nothing is picked.

    No button label breaks inside itself (plan §5b V2: «Удалить и отключить 3 / зависимых мода»):
    the labels never wrap, and when the buttons do not fit on one line the footer wraps whole
    buttons instead. Three buttons get a wider panel by default, so they sit on one line in RU and
    EN alike.
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
    confirmDisabled = false,
    error = null,
    confirmTestid,
    panelClass,
    secondaryLabel,
    secondaryBusy = false,
    secondaryTestid,
    onSecondary,
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
    /** The confirm's in-flight flag. */
    busy?: boolean;
    /** The confirm is off (e.g. a subset confirm with nothing ticked). */
    confirmDisabled?: boolean;
    error?: string | null;
    confirmTestid?: string;
    /** Defaults to the standard 440 px panel, 560 px when the secondary action shows. */
    panelClass?: string;
    /** Already-localized alternative between Cancel and the confirm; rendered only together with
     *  `onSecondary`. Always `.btn-secondary` — it is never the headline action. */
    secondaryLabel?: string;
    /** The secondary action's own in-flight flag: one `busy` cannot say which button spins. */
    secondaryBusy?: boolean;
    secondaryTestid?: string;
    onSecondary?: () => void;
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();

  const uid = crypto.randomUUID();
  const titleId = `confirm-title-${uid}`;
  const bodyId = `confirm-body-${uid}`;

  const paragraphs = $derived(
    bodyText == null ? [] : Array.isArray(bodyText) ? bodyText : [bodyText],
  );

  const confirmClass = $derived(
    `${variant === 'danger' ? 'btn-danger' : 'btn-primary'} btn-sm whitespace-nowrap`,
  );
  const resolvedCancel = $derived(cancelLabel ?? $t('common.cancel'));
  const anyBusy = $derived(busy || secondaryBusy);
  const showSecondary = $derived(secondaryLabel !== undefined && onSecondary !== undefined);
  // Three buttons need the room: in 440 px the longest RU confirm alone takes most of the line.
  const resolvedPanel = $derived(
    panelClass ??
      (showSecondary
        ? 'w-[560px] max-w-full p-5 flex flex-col gap-3'
        : 'w-[440px] p-5 flex flex-col gap-3'),
  );

  // DESIGN.md §8: a focused button that turns disabled drops focus to <body>, and Tab then walks
  // the page behind the dialog. On the rising edge of either busy flag, when focus has left the
  // dialog or sits on a now-disabled control, park it on the body (tabindex=-1, so trapFocus never
  // picks it as the initial stop). Callers that never set a busy flag are unaffected.
  let bodyEl = $state<HTMLDivElement | undefined>();
  // Plain variable: the effect's memory of the previous run, not state anything renders.
  let wasBusy = false;
  $effect(() => {
    const now = anyBusy;
    if (now && !wasBusy && bodyEl) {
      const active = document.activeElement;
      const inside = bodyEl.parentElement?.contains(active) ?? false;
      const stranded = active instanceof HTMLButtonElement && active.disabled;
      if (!inside || stranded) bodyEl.focus();
    }
    wasBusy = now;
  });
</script>

<Modal
  ariaLabelledby={titleId}
  ariaDescribedby={bodyId}
  onClose={onCancel}
  closeOnBackdrop={!anyBusy}
  closeOnEscape={!anyBusy}
  panelClass={resolvedPanel}
>
  <DialogTitle id={titleId} size={titleSize}>{title}</DialogTitle>

  <div id={bodyId} bind:this={bodyEl} tabindex="-1" class="flex flex-col gap-2 outline-none">
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

  <!-- Whole buttons wrap, never a label (see the header comment). -->
  <div class="flex flex-wrap justify-end gap-2 mt-2">
    <button
      type="button"
      class="btn-secondary btn-sm whitespace-nowrap"
      disabled={anyBusy}
      onclick={onCancel}
    >
      {resolvedCancel}
    </button>
    {#if showSecondary}
      <BusyButton
        busy={secondaryBusy}
        disabled={busy}
        type="button"
        class="btn-secondary btn-sm whitespace-nowrap"
        data-testid={secondaryTestid}
        onclick={onSecondary}
      >
        {secondaryLabel}
      </BusyButton>
    {/if}
    <BusyButton
      {busy}
      disabled={secondaryBusy || confirmDisabled}
      type="button"
      class={confirmClass}
      data-testid={confirmTestid}
      onclick={onConfirm}
    >
      {confirmLabel}
    </BusyButton>
  </div>
</Modal>
