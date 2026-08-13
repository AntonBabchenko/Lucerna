// Svelte action: trap keyboard focus inside a node while it is mounted, and
// restore focus to the previously-focused element when it unmounts.
//
// Generalises the inline Tab-trap the onboarding tour overlays already
// implement (TourOverlay.svelte / ContextualTour.svelte) and adds the
// focus-restore that dialogs need. Mount on a modal/drawer panel via
// `use:trapFocus`.
//
// Initial focus: the first descendant marked `[data-autofocus]`, else the
// first focusable descendant, else the node itself (give the node
// `tabindex="-1"` so this fallback works).

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

// A contextual onboarding tour (ContextualTour.svelte) renders its popover
// above the trapped panel and runs its own Tab-trap. While it is up, this
// modal trap must yield: it must neither pull initial focus back off the tour
// popover nor wrap Tab within the panel, or keyboard users get locked out of
// the tour. Gated on the body flag the tour sets while active.
const CTX_TOUR_ATTR = 'data-ctx-tour-active';

function ctxTourActive(): boolean {
  return typeof document !== 'undefined' && document.body.hasAttribute(CTX_TOUR_ATTR);
}

export function trapFocus(node: HTMLElement) {
  const restoreTo = document.activeElement as HTMLElement | null;

  // Armed only on the yield path below; disconnected on the first release and
  // on destroy, so the common case (no tour up) allocates nothing.
  let tourWatch: MutationObserver | null = null;

  function watchForTourRelease() {
    tourWatch = new MutationObserver(() => {
      if (ctxTourActive()) return;
      tourWatch?.disconnect();
      tourWatch = null;
      // Not `focusInitial()` unconditionally: the user may have clicked into
      // the panel while the tour was up, and pulling them to [data-autofocus]
      // would undo their own choice.
      if (!node.contains(document.activeElement)) focusInitial();
    });
    tourWatch.observe(document.body, { attributes: true, attributeFilter: [CTX_TOUR_ATTR] });
  }

  function focusInitial() {
    // If a contextual tour is already up when this panel mounts, leave focus
    // where the tour placed it rather than yanking it into the panel — and
    // then GIVE THE YIELD BACK. A tour ends by unmounting, which drops focus
    // to <body>; nothing else would ever move it into this panel, so the
    // dialog would sit unfocused (never announced to a screen reader) with its
    // node-scoped Tab handler unreachable — Tab would walk the application
    // behind the open dialog instead of cycling inside it.
    if (ctxTourActive() && !node.contains(document.activeElement)) {
      // Yield only when the way back exists. Without MutationObserver there is
      // no release signal, and a permanently focusless dialog is the worse of
      // the two failures — so take focus now and let the tour lose it.
      if (typeof MutationObserver === 'function') {
        watchForTourRelease();
        return;
      }
    }
    const preferred = node.querySelector<HTMLElement>('[data-autofocus]');
    (preferred ?? focusableDescendants(node)[0] ?? node).focus();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== 'Tab') return;
    // Let the active contextual tour own Tab while it is on screen.
    if (ctxTourActive()) return;
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

    // Defensive: the active element is somehow outside the panel. This should
    // not occur with a node-level listener (Tab only bubbles here while focus
    // is inside), but if focus is ever moved out programmatically, pull it
    // back to the edge rather than letting Tab escape.
    if (!active || !node.contains(active)) {
      e.preventDefault();
      (e.shiftKey ? last : first).focus();
      return;
    }
    // Wrap at the boundaries.
    if (e.shiftKey && active === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && active === last) {
      e.preventDefault();
      first.focus();
    }
  }

  // Defer initial focus until after the node is painted.
  let raf = 0;
  if (typeof requestAnimationFrame === 'function') {
    raf = requestAnimationFrame(focusInitial);
  } else {
    focusInitial();
  }
  node.addEventListener('keydown', onKeydown);

  return {
    destroy() {
      if (typeof cancelAnimationFrame === 'function') cancelAnimationFrame(raf);
      tourWatch?.disconnect();
      tourWatch = null;
      node.removeEventListener('keydown', onKeydown);
      // Restore focus to whatever was focused before the trap opened, if it is
      // still in the document and focusable.
      if (restoreTo && document.contains(restoreTo)) restoreTo.focus?.();
    },
  };
}
