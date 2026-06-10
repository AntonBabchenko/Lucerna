<script module lang="ts">
  // Stack of currently-open modals. Only the topmost responds to Escape, so a
  // nested modal (e.g. a delete-confirm opened on top of a detail modal) does
  // not close every layer with one keypress.
  let openStack: symbol[] = [];
</script>

<script lang="ts">
  // Shared accessible modal shell. Lifts the backdrop + centred panel +
  // role/aria-modal wiring + focus trap + focus restore + Escape / backdrop
  // close out of every individual dialog. Dialogs provide their own
  // header / body / footer as children.
  //
  // Labelling: pass exactly one of `ariaLabelledby` (id of a heading inside
  // the panel — preferred) or `ariaLabel` (literal string).
  //
  // Closing: Escape and a backdrop click both call `onClose`. Set
  // `closeOnBackdrop={false}` (e.g. while a destructive op is in flight) to
  // require an explicit button; `closeOnEscape={false}` likewise.
  import { onMount } from 'svelte';
  import type { Snippet } from 'svelte';
  import { trapFocus } from './trap-focus';

  let {
    onClose,
    ariaLabel,
    ariaLabelledby,
    panelClass = 'max-w-lg w-full',
    closeOnBackdrop = true,
    closeOnEscape = true,
    dataTestid,
    children,
  }: {
    onClose: () => void;
    ariaLabel?: string;
    ariaLabelledby?: string;
    panelClass?: string;
    closeOnBackdrop?: boolean;
    closeOnEscape?: boolean;
    /** Optional `data-testid` forwarded to the dialog panel element. */
    dataTestid?: string;
    children: Snippet;
  } = $props();

  // Register in the open-modal stack so only the topmost handles Escape.
  const id = Symbol('modal');
  onMount(() => {
    openStack.push(id);
    return () => {
      openStack = openStack.filter((s) => s !== id);
    };
  });
  const isTopmost = () => openStack[openStack.length - 1] === id;

  function onWindowKeydown(e: KeyboardEvent) {
    if (closeOnEscape && e.key === 'Escape' && isTopmost()) {
      e.stopPropagation();
      onClose();
    }
  }

  function onBackdropClick(e: MouseEvent) {
    // Only a click directly on the backdrop (not one bubbling up from the
    // panel) closes — so selecting text and releasing outside won't close.
    if (closeOnBackdrop && e.target === e.currentTarget) onClose();
  }
</script>

<svelte:window onkeydown={onWindowKeydown} />

<!-- Backdrop is a mouse convenience; keyboard users close via Escape, so it
     needs no key handler. -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
  onclick={onBackdropClick}
>
  <div
    use:trapFocus
    role="dialog"
    aria-modal="true"
    aria-label={ariaLabel}
    aria-labelledby={ariaLabelledby}
    data-testid={dataTestid}
    tabindex="-1"
    class="bg-surface rounded-lg shadow-xl outline-none {panelClass}"
  >
    {@render children()}
  </div>
</div>
