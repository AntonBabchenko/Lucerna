import type { ContextMenuItem } from '$lib/ui/menu-item';

const ROW_PX = 34;
// h-px rule + my-1 above and below.
const SEPARATOR_PX = 9;
// A reason wraps to two text-xs lines at the 220px menu width.
const REASON_PX = 32;
// py-1 + border on the menu surface.
const CHROME_PX = 10;

/** Pre-mount height estimate the menu triggers use to clamp a menu on screen. */
export function estimateMenuHeight(items: readonly ContextMenuItem[]): number {
  return items.reduce(
    (height, it) =>
      height +
      ROW_PX +
      (it.separatorBefore ? SEPARATOR_PX : 0) +
      (it.disabled && it.disabledReason ? REASON_PX : 0),
    CHROME_PX,
  );
}
