// src/lib/ui/tooltip/position.ts
// Pure tooltip positioning. No DOM access — fed a trigger rect, the measured
// bubble size, the requested placement, and the viewport, it returns absolute
// `fixed` coordinates clamped into the viewport, flipping top↔bottom when the
// preferred side has no room. Kept pure so it is unit-tested with plain numbers
// (happy-dom has no layout engine; real geometry is covered by e2e).

export type Placement = 'top' | 'bottom';

export interface Size {
  width: number;
  height: number;
}

export interface Viewport {
  width: number;
  height: number;
}

// A structural subset of DOMRect — only the fields we read.
export interface TriggerRect {
  top: number;
  left: number;
  width: number;
  height: number;
  bottom: number;
}

const GAP = 6; // px between trigger and bubble
const MARGIN = 8; // px minimum distance from any viewport edge

export function computePosition(
  trigger: TriggerRect,
  bubble: Size,
  placement: Placement,
  viewport: Viewport,
): { top: number; left: number; placement: Placement } {
  let actual = placement;
  let top: number;

  if (placement === 'top') {
    top = trigger.top - bubble.height - GAP;
    if (top < MARGIN) {
      actual = 'bottom';
      top = trigger.bottom + GAP;
    }
  } else {
    top = trigger.bottom + GAP;
    if (top + bubble.height > viewport.height - MARGIN) {
      actual = 'top';
      top = trigger.top - bubble.height - GAP;
    }
  }

  const centered = trigger.left + trigger.width / 2 - bubble.width / 2;
  const maxLeft = viewport.width - bubble.width - MARGIN;
  const left = Math.min(Math.max(centered, MARGIN), Math.max(MARGIN, maxLeft));

  return { top, left, placement: actual };
}
