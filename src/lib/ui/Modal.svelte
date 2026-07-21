<script module lang="ts">
  // Stack of currently-open modals, in mount order. Only the topmost responds
  // to Escape, so a nested modal (e.g. a delete-confirm opened on top of a
  // detail modal) does not close every layer with one keypress.
  //
  // Invariant: mount order == paint (DOM) order. All modals share one z-index
  // (z-50) and stack purely by DOM order, so the last-mounted modal is also the
  // visually-topmost one. This holds because stacked modals are always rendered
  // *after* their predecessors (a nested confirm sits after its parent in the
  // template; cross-component modals are ordered in +page.svelte). If a future
  // modal is placed earlier in the DOM but mounts later, Escape would close the
  // visually-lower one — keep new stacked modals after the ones they cover.
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
    ariaDescribedby,
    panelClass = 'max-w-lg w-full',
    closeOnBackdrop = true,
    closeOnEscape = true,
    bare = false,
    dataTestid,
    children,
  }: {
    onClose: () => void;
    ariaLabel?: string;
    ariaLabelledby?: string;
    /** Id of body copy to announce alongside the title — e.g. a confirm
        dialog's irreversibility warning. Without it screen readers announce
        only the heading. ConfirmDialog wires this automatically. */
    ariaDescribedby?: string;
    panelClass?: string;
    closeOnBackdrop?: boolean;
    closeOnEscape?: boolean;
    /** Full-bleed dialogs (the screenshot lightbox): the panel fills the
        viewport with no surface chrome, and the scrim darkens to bg-black/60.
        The panel covers the backdrop, so provide your own click-to-close
        surface inside if backdrop-click dismissal is wanted. Everything else
        (Escape stack, focus trap, role/aria) works as usual. */
    bare?: boolean;
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
    // A contextual onboarding tour (ContextualTour.svelte) renders its popover
    // above this modal but is NOT in openStack, so without this guard Escape
    // would close the host modal out from under the tour. While the tour is up,
    // its own window handler owns Escape (advance/dismiss the tour); the modal
    // stays open.
    if (document.body.hasAttribute('data-ctx-tour-active')) return;
    if (closeOnEscape && e.key === 'Escape' && isTopmost()) {
      onClose();
    }
  }

  // A backdrop dismissal must be a deliberate click *outside* the panel: the
  // press and the release both land directly on the backdrop. We track the
  // press origin instead of reacting to `click`, because a `click` fires on the
  // backdrop (the common ancestor) even when the press began inside the panel —
  // e.g. a drag text-selection released past the panel edge. Closing there would
  // silently discard the user's selection. Requiring both ends on the backdrop
  // also fixes the inverse: a genuine backdrop click is no longer blocked just
  // because some text happens to remain selected in the panel.
  let pressOnBackdrop = false;

  function onBackdropMouseDown(e: MouseEvent) {
    pressOnBackdrop = e.target === e.currentTarget;
  }

  function onBackdropMouseUp(e: MouseEvent) {
    const startedOnBackdrop = pressOnBackdrop;
    pressOnBackdrop = false;
    if (!closeOnBackdrop) return;
    if (startedOnBackdrop && e.target === e.currentTarget) onClose();
  }
</script>

<svelte:window onkeydown={onWindowKeydown} />

<!-- Backdrop is a mouse convenience; keyboard users close via Escape, so it
     needs no key handler. Dismissal uses mousedown+mouseup (not click) so it can
     require the press AND release to land on the backdrop. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class={bare
    ? 'fixed inset-0 z-50 bg-black/60'
    : 'fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4'}
  onmousedown={onBackdropMouseDown}
  onmouseup={onBackdropMouseUp}
>
  <div
    use:trapFocus
    role="dialog"
    aria-modal="true"
    aria-label={ariaLabel}
    aria-labelledby={ariaLabelledby}
    aria-describedby={ariaDescribedby}
    data-testid={dataTestid}
    tabindex="-1"
    class={bare
      ? `h-full w-full outline-none ${panelClass}`
      : `bg-surface rounded-lg shadow-xl outline-none ${panelClass}`}
  >
    {@render children()}
  </div>
</div>
