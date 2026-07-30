// Pure geometry for draggable pane splitters, kept DOM-free so the clamp is
// unit-testable. Shared by the skin editor (viewport ↔ tool panel) and the
// Manage-instances modal (instance list ↔ detail pane).

/** Keyboard nudge per arrow press, in px. */
export const SPLITTER_KEY_STEP = 16;

/** Clamp a proposed pane width (px) into the allowed range. */
export function clampPanelWidth(px: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, px));
}
