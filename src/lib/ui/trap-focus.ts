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

export function trapFocus(node: HTMLElement) {
  const restoreTo = document.activeElement as HTMLElement | null;

  function focusInitial() {
    const preferred = node.querySelector<HTMLElement>('[data-autofocus]');
    (preferred ?? focusableDescendants(node)[0] ?? node).focus();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== 'Tab') return;
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
      node.removeEventListener('keydown', onKeydown);
      // Restore focus to whatever was focused before the trap opened, if it is
      // still in the document and focusable.
      if (restoreTo && document.contains(restoreTo)) restoreTo.focus?.();
    },
  };
}
