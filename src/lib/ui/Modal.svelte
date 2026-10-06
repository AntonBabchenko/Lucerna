<script lang="ts">
  // Shared accessible modal shell. Lifts the backdrop + centred panel +
  // role/aria-modal wiring + focus trap + focus restore + Escape / backdrop
  // close out of every individual dialog. Dialogs provide their own
  // header / body / footer as children.
  //
  // A dialog that swaps its content (a wizard's step, a flow's phase) passes
  // `stepKey`: when it changes and the focus has fallen to <body> — the control
  // that held it left with the old content — the panel takes it (DESIGN.md §8).
  //
  // Labelling: pass exactly one of `ariaLabelledby` (id of a heading inside
  // the panel — preferred) or `ariaLabel` (literal string).
  //
  // Closing: Escape and a backdrop click both call `onClose`. Set
  // `closeOnBackdrop={false}` (e.g. while a destructive op is in flight) to
  // require an explicit button; `closeOnEscape={false}` likewise. A press on
  // the backdrop never moves the focus; a backdrop dismissal leaves the
  // focused field first.
  import { onDestroy, untrack } from 'svelte';
  import type { Snippet } from 'svelte';
  import {
    isTopModal,
    newLayerId,
    provideLayerHost,
    pushLayer,
    tourAbove,
  } from './layer-stack.svelte';
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
    stepKey,
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
    /** A value naming what the dialog shows — its step, its phase. When it changes and the
        focus has fallen to <body>, the panel takes it. Omitted: the dialog never swaps its
        content. */
    stepKey?: unknown;
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

  // A step change takes the focused control with the old content: the focus
  // falls to <body>, and the new step opens with nothing focused — nothing
  // announced, Enter doing nothing. After the update (effects run after the
  // DOM), the panel takes a focus nothing holds; the panel is the labelled
  // element, so the new step's title is read out, and Tab goes on from it
  // (trapFocus). Only on a CHANGE — at open, trapFocus places the initial focus
  // — and only for the topmost dialog with no tour above it, the rule of
  // trapFocus's Tab from <body>: a focus something holds (a toast's button, a
  // tour's card) is never taken. Plain variable: an effect-local memory.
  let panel = $state<HTMLDivElement | undefined>();
  let shownStep: unknown = untrack(() => stepKey);
  $effect(() => {
    const key = stepKey;
    if (key === shownStep) return;
    shownStep = key;
    untrack(() => {
      const active = document.activeElement;
      if (active !== null && active !== document.body) return;
      if (!isTopModal(layer) || tourAbove(layer)) return;
      panel?.focus();
    });
  });

  // A backdrop dismissal must be a deliberate click *outside* the panel: the
  // press and the release both land directly on the backdrop. We track the
  // press origin instead of reacting to `click`, because a `click` fires on the
  // backdrop (the common ancestor) even when the press began inside the panel —
  // e.g. a drag text-selection released past the panel edge. Closing there would
  // silently discard the user's selection. Requiring both ends on the backdrop
  // also fixes the inverse: a genuine backdrop click is no longer blocked just
  // because some text happens to remain selected in the panel. The press
  // itself never moves the focus (onBackdropMouseDown).
  let pressOnBackdrop = false;

  function onBackdropMouseDown(e: MouseEvent) {
    pressOnBackdrop = e.target === e.currentTarget;
    // The scrim is no place for the focus. Not focusable, a press on it would
    // drop the focus to <body> — the field's caret and the user's place in the
    // dialog gone, the field's blur handlers run — even when the release lands
    // in the panel and nothing closes. Keep the focus where it is. (Nor does
    // the press start a text selection: there is nothing on the scrim to
    // select.)
    if (pressOnBackdrop) e.preventDefault();
  }

  function onBackdropMouseUp(e: MouseEvent) {
    const startedOnBackdrop = pressOnBackdrop;
    pressOnBackdrop = false;
    if (!closeOnBackdrop) return;
    if (!startedOnBackdrop || e.target !== e.currentTarget) return;
    // A click outside leaves the focused field, as the press did before it
    // kept the focus: a field that saves when it is left (the instance name in
    // Manage) saves, then the dialog closes.
    const scrim = e.currentTarget as HTMLElement;
    const active = document.activeElement;
    if (active instanceof HTMLElement && scrim.contains(active)) active.blur();
    onClose();
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
    bind:this={panel}
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
