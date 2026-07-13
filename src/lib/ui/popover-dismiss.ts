// Shared close-on-scroll/resize wiring for FIXED popovers/menus, extracted so
// every overlay dismisses the same way instead of re-declaring the listeners.
// A position:fixed popover is measured from its trigger on open and does NOT
// follow it when the layout shifts, so any ancestor scroll or a window resize
// must close it. Placement maths lives in select-placement.ts; this is only the
// dismiss glue. (Absolute-positioned dropdowns that move with their container —
// e.g. McVersionCombobox — don't need this.)

export interface PopoverDismissOptions {
  /** Called when the popover should close. */
  onDismiss: () => void;
  /**
   * When provided, a scroll whose target is inside this element is ignored, so
   * wheeling a long internal list (or an arrow-key scrollIntoView) does not
   * dismiss the popover. Lists that scroll internally pass their list element;
   * point popovers with no scrollable body omit it.
   */
  ignoreScrollWithin?: () => Node | null | undefined;
}

/**
 * Attach the dismiss listeners and return a cleanup that detaches them. Call
 * from an $effect / onMount and return the cleanup:
 *
 *   $effect(() => (open ? attachPopoverDismiss({ onDismiss: close }) : undefined));
 */
export function attachPopoverDismiss(opts: PopoverDismissOptions): () => void {
  const { onDismiss, ignoreScrollWithin } = opts;
  // Capture-phase scroll (3rd arg true) so a scrollable ancestor's own scroll —
  // which does not bubble — is caught too. Capture also delivers scroll from
  // DESCENDANTS, hence the ignoreScrollWithin guard for internally-scrolling lists.
  const onScroll = (e: Event) => {
    if (ignoreScrollWithin?.()?.contains(e.target as Node)) return;
    onDismiss();
  };
  const onResize = () => onDismiss();
  window.addEventListener('scroll', onScroll, true);
  window.addEventListener('resize', onResize);
  return () => {
    window.removeEventListener('scroll', onScroll, true);
    window.removeEventListener('resize', onResize);
  };
}
