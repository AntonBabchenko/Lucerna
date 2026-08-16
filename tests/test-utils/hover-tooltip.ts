// tests/test-utils/hover-tooltip.ts
// Pointer-hover a `use:tooltip` trigger and let the shared controller's open
// delay elapse.
//
// The action's mouseenter path is deliberately NOT immediate — only keyboard
// focus is (`tooltip.ts`: `open(false)` on mouseenter vs `open(true)` on
// focusin), so `showTooltip` schedules the reveal `OPEN_DELAY_MS` later. A bare
// `dispatchEvent(new MouseEvent('mouseenter'))` therefore leaves
// `tooltipState.visible === false`, and an assertion on the next line reads the
// pre-open state rather than the tooltip.
//
// Focus cannot stand in for hover in a component test: the action gates focus
// on `:focus-visible`, which happy-dom cannot model — tests/tooltip/
// tooltip-action.test.ts stubs `node.matches` for exactly that reason, which
// only works when the test owns the node.
//
// Fake timers are installed around the dispatch alone. The timeout is scheduled
// by the dispatch itself, so nothing set up earlier in the test is affected, and
// real timers are restored before returning. Same advance-the-open-delay shape
// as tests/tooltip/tooltip-controller.test.ts and tests/log-hint-hover.test.ts.
import { vi } from 'vitest';
import { OPEN_DELAY_MS } from '$lib/ui/tooltip/tooltip-controller.svelte';

export function hoverTooltip(node: Element): void {
  vi.useFakeTimers();
  try {
    node.dispatchEvent(new MouseEvent('mouseenter'));
    vi.advanceTimersByTime(OPEN_DELAY_MS);
  } finally {
    vi.useRealTimers();
  }
}

/** Pointer-leave counterpart — the action's mouseleave path hides immediately. */
export function unhoverTooltip(node: Element): void {
  node.dispatchEvent(new MouseEvent('mouseleave'));
}
