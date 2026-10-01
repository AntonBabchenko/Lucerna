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
  import { onDestroy, untrack } from 'svelte';
  import type { Snippet } from 'svelte';
  import { newLayerId, provideLayerHost, pushLayer } from './layer-stack.svelte';
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
    takesFileDrops = false,
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
    /** The dialog's body takes OS file drops (the window drop router routes them to it — the
        Modpacks modal). Any other dialog on top leaves every drop box under it out. Read once,
        when the dialog opens. */
    takesFileDrops?: boolean;
    /** Optional `data-testid` forwarded to the dialog panel element. */
    dataTestid?: string;
    children: Snippet;
  } = $props();

  // One entry in the app's layer stack (layer-stack.svelte.ts): Escape reaches
  // this modal only while it is the top layer (a popover or a nested dialog
  // opened over it takes the key first), a contextual tour rendered among its
  // children is hosted by it, and the window drop router asks the stack whether
  // the topmost modal takes a file drop (`modalBlocksFileDrops`).
  //
  // Pushed during INITIALISATION, not in onMount: a tour among the children
  // looks its host up in its own onMount, and a child's mount callbacks run
  // before its parent's. Stack order still equals paint order: all modals share
  // z-50 and stack purely by DOM order, and a modal rendered after another both
  // initialises and paints after it (a nested confirm sits after its parent in
  // the template; cross-component modals are ordered in +page.svelte). If a
  // future modal is placed earlier in the DOM but opens later, Escape would
  // close the visually-lower one — and a drop would follow the hidden one's
  // rule — so keep new stacked modals after the ones they cover.
  const layer = newLayerId('modal');
  onDestroy(
    pushLayer(
      layer,
      'modal',
      () => {
        if (closeOnEscape) onClose();
      },
      // Read once, as the dialog opens; `untrack` marks the one-time read.
      { takesFileDrops: untrack(() => takesFileDrops) },
    ),
  );
  provideLayerHost(layer);

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
    use:trapFocus={layer}
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
