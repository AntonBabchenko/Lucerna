// src/lib/ui/tooltip/tooltip-controller.svelte.ts
// Single source of truth for the one tooltip that may be visible at a time.
// Module-level `$state` + exported functions (mirrors src/lib/i18n/state.svelte.ts).
// The `use:tooltip` action calls show/hide; TooltipLayer reads `tooltipState`,
// measures the rendered bubble, and calls positionTooltip() to finalize coords.
import { computePosition, type Placement, type Size, type TriggerRect } from './position';

/** Shared DOM id of the single bubble — set as aria-describedby on triggers. */
export const TOOLTIP_ID = 'app-tooltip';

/** Hover open delay. Keyboard focus shows immediately (a11y), bypassing this. */
export const OPEN_DELAY_MS = 400;

export const tooltipState = $state<{
  visible: boolean;
  text: string;
  top: number;
  left: number;
  placement: Placement;
}>({ visible: false, text: '', top: 0, left: 0, placement: 'top' });

let triggerRect: TriggerRect | null = null;
let openTimer: ReturnType<typeof setTimeout> | null = null;

function clearTimer() {
  if (openTimer !== null) {
    clearTimeout(openTimer);
    openTimer = null;
  }
}

function onDismiss() {
  hideTooltip();
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') hideTooltip();
}

function attachDismiss() {
  // Capture scroll so a scrollable host's own scroll (which does not bubble) is
  // caught too — same approach as HelpPopover.
  window.addEventListener('scroll', onDismiss, true);
  window.addEventListener('resize', onDismiss);
  window.addEventListener('keydown', onKeydown);
}

function detachDismiss() {
  window.removeEventListener('scroll', onDismiss, true);
  window.removeEventListener('resize', onDismiss);
  window.removeEventListener('keydown', onKeydown);
}

export interface ShowOptions {
  placement: Placement;
  immediate: boolean;
}

export function showTooltip(rect: TriggerRect, text: string, opts: ShowOptions): void {
  clearTimer();
  triggerRect = rect;
  const reveal = () => {
    tooltipState.text = text;
    tooltipState.placement = opts.placement;
    tooltipState.visible = true;
    attachDismiss();
  };
  if (opts.immediate) reveal();
  else openTimer = setTimeout(reveal, OPEN_DELAY_MS);
}

export function hideTooltip(): void {
  clearTimer();
  if (tooltipState.visible) detachDismiss();
  tooltipState.visible = false;
  triggerRect = null;
}

/** Called by TooltipLayer once it has measured its own rendered size. */
export function positionTooltip(bubble: Size): void {
  if (!triggerRect) return;
  const r = computePosition(triggerRect, bubble, tooltipState.placement, {
    width: window.innerWidth,
    height: window.innerHeight,
  });
  tooltipState.top = r.top;
  tooltipState.left = r.left;
  tooltipState.placement = r.placement;
}
