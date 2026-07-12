// Pure geometry for the editor's draggable 3D↔panel splitter, extracted so the
// clamp logic is unit-testable without a DOM. The panel lives on the right; a
// larger width shrinks the 3D column (which is flex-1 and fills the remainder).
import { SKIN_SIZE } from './buffer';

export const PANEL_MIN_WIDTH = 240;
export const PANEL_MAX_WIDTH = 640;
export const PANEL_KEY_STEP = 16;

// Backing resolution bounds for the companion canvas (texel size in device px).
// The canvas is CSS-scaled to the panel width; the backing stays a multiple of
// SKIN_SIZE so the grid and mirror fold-lines render crisp 1px strokes.
export const MIN_CELL = 1;
export const MAX_CELL = 12;

/** Clamp a proposed panel width (px) into the allowed range. */
export function clampPanelWidth(
  px: number,
  min: number = PANEL_MIN_WIDTH,
  max: number = PANEL_MAX_WIDTH,
): number {
  return Math.min(max, Math.max(min, px));
}

/** Integer texel size (backing px per texel) for a companion box `px` wide. */
export function companionCell(px: number): number {
  return Math.min(MAX_CELL, Math.max(MIN_CELL, Math.floor(px / SKIN_SIZE)));
}

/** Double-click behaviour: snap to `max`, or return `restore` when already maxed. */
export function toggleMaxWidth(
  current: number,
  restore: number,
  max: number = PANEL_MAX_WIDTH,
): number {
  return current >= max ? restore : max;
}
