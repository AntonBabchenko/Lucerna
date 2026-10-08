// Where a contextual tour's card goes for a spotlight rect (ContextualTour). Pure, so the
// geometry is testable without a layout engine.

export const POPOVER_WIDTH = 320;
export const MARGIN = 16;
// What a card is assumed to need below or above its anchor. A card is taller with longer copy; the
// budget only decides the side, the card's own height does the rest.
export const POPOVER_HEIGHT_BUDGET = 220;
const GAP = 12;

/** Inline style for the card: beside / below its anchor, or centred when there is none. */
export function popoverStyle(r: DOMRect | null, anchor: string, vw: number, vh: number): string {
  if (!r) {
    return 'top:50%; left:50%; transform:translate(-50%,-50%);';
  }
  if (anchor === 'right') {
    const leftPart =
      r.right + MARGIN + POPOVER_WIDTH + MARGIN <= vw
        ? `left:${r.right + MARGIN}px;`
        : `left:${Math.max(MARGIN, r.left - POPOVER_WIDTH - MARGIN)}px;`;
    const midY = r.top + r.height / 2;
    const vertical =
      midY > vh / 2 ? `bottom:${Math.max(MARGIN, vh - r.bottom)}px;` : `top:${r.top}px;`;
    return `${vertical} ${leftPart}`;
  }
  if (anchor === 'below') {
    let leftCoord = r.left;
    if (leftCoord + POPOVER_WIDTH + MARGIN > vw) {
      leftCoord = Math.max(MARGIN, vw - POPOVER_WIDTH - MARGIN);
    }
    // Flip above the anchor when there isn't room below it. A `below`-anchored
    // step near the viewport bottom (e.g. the manage-instances actions row)
    // would otherwise position the popover off the bottom edge — invisible,
    // leaving only a dimmed screen with no reachable controls.
    const fitsBelow = r.bottom + GAP + POPOVER_HEIGHT_BUDGET <= vh;
    if (fitsBelow) {
      return `top:${r.bottom + GAP}px; left:${leftCoord}px;`;
    }
    // Above, when the card's top stays inside the window there. Its own threshold (0, not MARGIN)
    // keeps every placement that was on screen before exactly where it was.
    const fitsAbove = r.top - GAP - POPOVER_HEIGHT_BUDGET >= 0;
    if (fitsAbove) {
      return `bottom:${Math.max(MARGIN, vh - r.top + GAP)}px; left:${leftCoord}px;`;
    }
    // Neither: an anchor nearly as tall as the window. Pinned to the bottom edge, the card covers
    // the lower part of the spotlight; its heading (the anchor's top) stays in view.
    return `bottom:${MARGIN}px; left:${leftCoord}px;`;
  }
  return 'top:50%; left:50%; transform:translate(-50%,-50%);';
}
