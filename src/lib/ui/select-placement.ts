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
  width: number;
  /** Height ceiling for the list (it scrolls internally beyond this). */
  maxHeight: number;
}

export function computePopoverPlacement(
  trigger: TriggerRect,
  viewport: Viewport,
  opts: PlacementOpts,
): Placement {
  const { gap, margin, maxHeight } = opts;

  const maxLeft = viewport.width - trigger.width - margin;
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
    width: trigger.width,
    maxHeight: Math.max(0, Math.min(maxHeight, space)),
  };
}
