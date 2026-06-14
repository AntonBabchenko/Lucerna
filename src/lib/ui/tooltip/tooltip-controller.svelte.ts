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

/** Keep the caret clear of the bubble's rounded corners when edge-clamped. */
const CARET_INSET = 12;

export const tooltipState = $state<{
  visible: boolean;
  text: string;
  top: number;
  left: number;
  placement: Placement;
  /** Caret centre, in px from the bubble's left edge — points at the trigger. */
  caretLeft: number;
}>({ visible: false, text: '', top: 0, left: 0, placement: 'top', caretLeft: CARET_INSET });

let triggerRect: TriggerRect | null = null;
let openTimer: ReturnType<typeof setTimeout> | null = null;
// Identity of the trigger that owns the currently-shown (or pending) tooltip.
// Lets hideTooltip ignore a hide request from a *different* trigger (e.g. a
// reactive update(null)/destroy elsewhere) so it cannot tear down this one.
let owner: unknown = null;

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
  /** The trigger that owns this tooltip; passed back to hideTooltip to scope hides. */
  owner?: unknown;
}

export function showTooltip(rect: TriggerRect, text: string, opts: ShowOptions): void {
  clearTimer();
  triggerRect = rect;
  owner = opts.owner ?? null;
  const reveal = () => {
    tooltipState.text = text;
    tooltipState.placement = opts.placement;
    tooltipState.visible = true;
    attachDismiss();
  };
  if (opts.immediate) reveal();
  else openTimer = setTimeout(reveal, OPEN_DELAY_MS);
}

export function hideTooltip(requester?: unknown): void {
  // A hide from a specific trigger only applies to the tooltip it owns. The
  // global dismiss path (scroll/resize/Escape) calls hideTooltip() with no
  // argument and always hides. This stops one trigger's reactive
  // update(null)/destroy from closing a different trigger's visible tooltip.
  if (requester !== undefined && requester !== owner) return;
  clearTimer();
  if (tooltipState.visible) detachDismiss();
  tooltipState.visible = false;
  triggerRect = null;
  owner = null;
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
  // The caret points at the trigger's centre, clamped within the bubble so it
  // stays clear of the rounded corners even when the bubble is edge-clamped.
  const triggerCenterX = triggerRect.left + triggerRect.width / 2;
  const maxCaret = Math.max(CARET_INSET, bubble.width - CARET_INSET);
  tooltipState.caretLeft = Math.min(Math.max(triggerCenterX - r.left, CARET_INSET), maxCaret);
}
