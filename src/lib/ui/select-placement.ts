// Pure placement maths for the Select popover, extracted so the viewport-clamp
// behaviour is unit-testable without a live DOM. The popover is position:fixed,
// so it is bounded by the viewport (in compact mode the OS window shrinks to the
// sidebar's content height — a short viewport). We clamp the list height to the
// room actually available on the chosen side and let it scroll internally rather
// than spill past the window edge and get clipped.

export interface TriggerRect {
  top: number;
  bottom: number;
  left: number;
  width: number;
}

export interface Viewport {
  width: number;
  height: number;
}

export interface PlacementOpts {
  /** Gap between trigger and popover. */
  gap: number;
  /** Keep-off-the-edge viewport margin. */
  margin: number;
  /** Preferred max height; the result is clamped down to the available room. */
  maxHeight: number;
}

export interface Placement {
  /** Open upward (anchored at the trigger top) instead of downward. */
  flipUp: boolean;
  /** When flipping down: the popover's top. When flipping up: the popover's
   *  bottom is anchored here (caller computes `bottom = viewport.height - top`). */
  top: number;
  left: number;
  /** Min-width (matches the trigger, but never wider than the viewport). */
  width: number;
  /** Max-width ceiling: long option labels truncate instead of growing the
   *  popover past the viewport edge. In a narrow window (the compact strip) this
   *  keeps right-aligned per-row content (e.g. a trash button) on screen. */
  maxWidth: number;
  /** Height ceiling for the list (it scrolls internally beyond this). */
  maxHeight: number;
}

export function computePopoverPlacement(
  trigger: TriggerRect,
  viewport: Viewport,
  opts: PlacementOpts,
): Placement {
  const { gap, margin, maxHeight } = opts;

  // Never let the popover be wider than the viewport (minus margins). Without
  // this, a long option label (white-space: nowrap from truncate) grows the
  // popover past a narrow window's right edge and clips right-aligned row
  // content. The min-width still matches the trigger, capped to the same ceiling.
  const maxWidth = Math.max(0, viewport.width - 2 * margin);
  const width = Math.min(trigger.width, maxWidth);
  const maxLeft = viewport.width - width - margin;
  const left = Math.min(Math.max(trigger.left, margin), Math.max(margin, maxLeft));

  const spaceBelow = viewport.height - trigger.bottom - gap - margin;
  const spaceAbove = trigger.top - gap - margin;
  // Open downward unless the room below can't hold the preferred height AND the
  // room above is more generous.
  const flipUp = spaceBelow < maxHeight && spaceAbove > spaceBelow;
  const space = flipUp ? spaceAbove : spaceBelow;

  return {
    flipUp,
    top: flipUp ? trigger.top - gap : trigger.bottom + gap,
    left,
    width,
    maxWidth,
    maxHeight: Math.max(0, Math.min(maxHeight, space)),
  };
}
