// Pure geometry for the editor's draggable 3D↔panel splitter, extracted so the
// clamp logic is unit-testable without a DOM. The panel lives on the right; a
// larger width shrinks the 3D column (which is flex-1 and fills the remainder).
export const PANEL_MIN_WIDTH = 240;
export const PANEL_MAX_WIDTH = 520;
export const PANEL_KEY_STEP = 16;

/** Clamp a proposed panel width (px) into the allowed range. */
export function clampPanelWidth(
  px: number,
  min: number = PANEL_MIN_WIDTH,
  max: number = PANEL_MAX_WIDTH,
): number {
  return Math.min(max, Math.max(min, px));
}
