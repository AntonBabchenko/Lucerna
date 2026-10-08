// Where a contextual tour's card goes for a spotlight rect (ContextualTour). Pure, so the
// geometry is testable without a layout engine.

export const POPOVER_WIDTH = 320;
export const MARGIN = 16;
// What a card is assumed to need below or above its anchor until its own height is known (the
// first frame, before it has rendered). Long copy makes a card taller than this — the RU steps of
// the deps tour are 250–290 px — so the measured height decides the side as soon as there is one.
export const POPOVER_HEIGHT_BUDGET = 220;
const GAP = 12;

/**
 * Inline style for the card: beside / below its anchor, or centred when there is none. `cardH` is
 * the card's rendered height; 0 (not measured yet) falls back to the budget.
 */
export function popoverStyle(
  r: DOMRect | null,
  anchor: string,
  vw: number,
  vh: number,
  cardH = 0,
): string {
  const height = cardH > 0 ? cardH : POPOVER_HEIGHT_BUDGET;
  if (!r) {
    return 'top:50%; left:50%; transform:translate(-50%,-50%);';
  }
  if (anchor === 'right') {
    const leftPart =
      r.right + MARGIN + POPOVER_WIDTH + MARGIN <= vw
        ? `left:${r.right + MARGIN}px;`
        : `left:${Math.max(MARGIN, r.left - POPOVER_WIDTH - MARGIN)}px;`;
    const midY = r.top + r.height / 2;
    // `top` never above the window: an anchor in a list the user scrolled during the tour can sit
    // far above it, and the card must keep its buttons in reach.
    const vertical =
      midY > vh / 2
        ? `bottom:${Math.max(MARGIN, vh - r.bottom)}px;`
        : `top:${Math.max(MARGIN, r.top)}px;`;
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
    const fitsBelow = r.bottom + GAP + height <= vh;
    if (fitsBelow) {
      // Never above the window's top edge (an anchor scrolled up out of view, see `right`).
      return `top:${Math.max(MARGIN, r.bottom + GAP)}px; left:${leftCoord}px;`;
    }
    // Above, when the card's top stays inside the window there. Its own threshold (0, not MARGIN)
    // keeps every placement that was on screen before exactly where it was.
    const fitsAbove = r.top - GAP - height >= 0;
    if (fitsAbove) {
      return `bottom:${Math.max(MARGIN, vh - r.top + GAP)}px; left:${leftCoord}px;`;
    }
    // Neither: an anchor nearly as tall as the window. Pinned to the bottom edge, the card covers
    // the lower part of the spotlight; its heading (the anchor's top) stays in view.
    return `bottom:${MARGIN}px; left:${leftCoord}px;`;
  }
  return 'top:50%; left:50%; transform:translate(-50%,-50%);';
}
