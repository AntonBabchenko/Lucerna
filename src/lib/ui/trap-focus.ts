// Svelte action: trap keyboard focus inside a node while it is mounted, and
// restore focus to the previously-focused element when it unmounts.
//
// Generalises the inline Tab-trap the onboarding tour overlays already
// implement (TourOverlay.svelte / ContextualTour.svelte) and adds the
// focus-restore that dialogs need. Mount on a modal panel via
// `use:trapFocus={layerId}` — the panel's entry in the layer stack.
//
// Initial focus: the first descendant marked `[data-autofocus]`, else the
// first focusable descendant, else the node itself (give the node
// `tabindex="-1"` so this fallback works).
//
// Tab: wraps at the panel's edges; a Tab pressed while nothing holds the focus
// goes to the topmost dialog (below, `onDocumentKeydown`).

import { untrack } from 'svelte';
import {
  hostsTour,
  isTopModal,
  type LayerId,
  onLayersChange,
  tourAbove,
} from './layer-stack.svelte';

const FOCUSABLE_SELECTOR = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

function focusableDescendants(node: HTMLElement): HTMLElement[] {
  return Array.from(node.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    // Skip elements hidden via display:none (offsetParent is null for those).
    // `offsetParent` is also null for position:fixed, so keep an element that
    // currently holds focus regardless. In a layout-less test DOM offsetParent
    // is always null; the real Tab-order behaviour is covered by an e2e test.
    (el) => el.offsetParent !== null || el === document.activeElement,
  );
}

// A contextual tour hosted by this dialog (ContextualTour.svelte) sits above it
// in the layer stack and runs its own Tab-trap on its card. While that tour is
// the top layer this trap yields: it neither pulls initial focus off the tour
// card nor wraps Tab inside the panel. It yields to nothing else — a popover
// opened inside the panel is a DOM descendant this trap already covers, and a
// page tour BELOW this dialog is not on screen at all.
export function trapFocus(node: HTMLElement, layer: LayerId) {
  const restoreTo = document.activeElement as HTMLElement | null;

  // The yield has to be GIVEN BACK. A tour hosted by this dialog moves the
  // focus to its card, and ends by unmounting it, which drops focus to <body>;
  // nothing else would ever move it into this panel, so the dialog would sit
  // unfocused (never announced to a screen reader) with its node-scoped Tab
  // handler unreachable — Tab would walk the application behind the open
  // dialog instead of cycling inside it. So for as long as the trap is mounted
  // it watches the stack: when a tour of this dialog arrives it notes where the
  // focus was in the panel, and when the tour is gone it puts the focus back
  // there (or on the initial target). For the trap's whole life, not only for a
  // tour already up when the dialog opens: one also arrives later — once the
  // dialog's content has loaded, or from a button inside it. A tour stepping
  // aside under something opened over it is not an end: it is still hosted.
  // Untracked: the action runs as its element mounts, and reading the stack
  // must not make anything around it depend on the stack.
  let touring = untrack(() => hostsTour(layer));
  let resumeAt: HTMLElement | null = null;
  let destroyed = false;
  const stopWatching = onLayersChange(() => {
    const now = hostsTour(layer);
    if (now === touring) return;
    touring = now;
    if (now) {
      const active = document.activeElement;
      resumeAt = active instanceof HTMLElement && node.contains(active) ? active : null;
    } else {
      giveFocusBack();
    }
  });

  function giveFocusBack() {
    if (!node.contains(document.activeElement)) {
      resume();
      return;
    }
    // Focus is still in the panel: the user clicked into it while the tour was
    // up — theirs, keep it — or it sits on the tour's card, a child of this
    // panel that may still be mounted when the tour's layer goes. Look again
    // once that update is over: the card is gone then, and so is the focus.
    queueMicrotask(() => {
      if (!destroyed && !touring && !node.contains(document.activeElement)) resume();
    });
  }

  function resume() {
    const at = resumeAt;
    resumeAt = null;
    if (at?.isConnected && node.contains(at)) at.focus();
    // Gone, or no longer focusable (disabled or hidden meanwhile): the initial target.
    if (document.activeElement !== at) focusInitial();
  }

  function focusInitial() {
    // A deep link may already have placed focus inside the dialog (fieldFlash
    // with focus, from a banner): keep it. A plain open still lands on
    // [data-autofocus] below.
    if (node.contains(document.activeElement)) return;
    // A tour on top of this dialog keeps the focus on its card; the watcher
    // above gives it back when the tour ends.
    if (tourAbove(layer)) return;
    const preferred = node.querySelector<HTMLElement>('[data-autofocus]');
    (preferred ?? focusableDescendants(node)[0] ?? node).focus();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== 'Tab') return;
    // The tour on top of this dialog owns Tab while it is on screen.
    if (tourAbove(layer)) return;
    const items = focusableDescendants(node);
    const active = document.activeElement as HTMLElement | null;

    // No focusable children: keep focus on the panel itself.
    if (items.length === 0) {
      e.preventDefault();
      node.focus();
      return;
    }

    const first = items[0];
    const last = items[items.length - 1];

    // Wrap at the boundaries. (This listener sits on the panel, so it sees a
    // Tab only while the focus is inside it; a Tab with the focus nowhere is
    // the document listener's below.)
    if (e.shiftKey && active === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && active === last) {
      e.preventDefault();
      first.focus();
    }
  }

  // A Tab pressed while nothing holds the focus — `<body>` — never reaches the
  // panel: a key event goes to the focused element, and there is none. The
  // focus gets there when what held it goes away under it: a toast above the
  // dialog that removed itself (its Undo, its ×), a focused control that is
  // removed or turns disabled. The browser would then go on from where that
  // stood — for a toast, and from the panel's edges, the page behind the
  // dialog. So the topmost dialog takes that press: Tab to its first control,
  // Shift+Tab to its last, the panel itself when it has none. Not while a tour
  // lies above it (the tour owns Tab), and never a focus that something holds
  // — a tour's card, a toast's button: that belongs to whoever has it.
  function onDocumentKeydown(e: KeyboardEvent) {
    if (e.key !== 'Tab' || e.defaultPrevented) return;
    const active = document.activeElement;
    if (active !== null && active !== document.body) return;
    if (!isTopModal(layer) || tourAbove(layer)) return;
    e.preventDefault();
    const items = focusableDescendants(node);
    const target = e.shiftKey ? items[items.length - 1] : items[0];
    (target ?? node).focus();
  }

  // Defer initial focus until after the node is painted.
  let raf = 0;
  if (typeof requestAnimationFrame === 'function') {
    raf = requestAnimationFrame(focusInitial);
  } else {
    focusInitial();
  }
  node.addEventListener('keydown', onKeydown);
  document.addEventListener('keydown', onDocumentKeydown);

  return {
    destroy() {
      if (typeof cancelAnimationFrame === 'function') cancelAnimationFrame(raf);
      destroyed = true;
      stopWatching();
      node.removeEventListener('keydown', onKeydown);
      document.removeEventListener('keydown', onDocumentKeydown);
      // Restore focus to whatever was focused before the trap opened, if it is
      // still in the document and focusable.
      if (restoreTo && document.contains(restoreTo)) restoreTo.focus?.();
    },
  };
}
