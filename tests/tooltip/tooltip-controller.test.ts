// tests/tooltip/tooltip-controller.test.ts
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  hideTooltip,
  OPEN_DELAY_MS,
  positionTooltip,
  showTooltip,
  tooltipState,
} from '$lib/ui/tooltip/tooltip-controller.svelte';

const rect = { top: 100, left: 100, width: 40, height: 20, bottom: 120 } as DOMRect;

afterEach(() => {
  hideTooltip();
  vi.useRealTimers();
});

describe('tooltip-controller', () => {
  beforeEach(() => vi.useFakeTimers());

  it('shows after the open delay on a non-immediate request', () => {
    showTooltip(rect, 'Hello', { placement: 'top', immediate: false });
    expect(tooltipState.visible).toBe(false);
    vi.advanceTimersByTime(OPEN_DELAY_MS);
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toBe('Hello');
  });

  it('shows synchronously on an immediate request (keyboard focus)', () => {
    showTooltip(rect, 'Now', { placement: 'top', immediate: true });
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toBe('Now');
  });

  it('a hide before the delay elapses cancels the pending show', () => {
    showTooltip(rect, 'Hello', { placement: 'top', immediate: false });
    hideTooltip();
    vi.advanceTimersByTime(OPEN_DELAY_MS);
    expect(tooltipState.visible).toBe(false);
  });

  it('dismisses on a captured window scroll', () => {
    showTooltip(rect, 'Hello', { placement: 'top', immediate: true });
    expect(tooltipState.visible).toBe(true);
    window.dispatchEvent(new Event('scroll'));
    expect(tooltipState.visible).toBe(false);
  });

  it('dismisses on Escape', () => {
    showTooltip(rect, 'Hello', { placement: 'top', immediate: true });
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(tooltipState.visible).toBe(false);
  });

  it('positionTooltip writes clamped coordinates from the trigger rect', () => {
    showTooltip(rect, 'Hello', { placement: 'top', immediate: true });
    positionTooltip({ width: 120, height: 30 });
    expect(typeof tooltipState.top).toBe('number');
    expect(typeof tooltipState.left).toBe('number');
    // Caret points at the trigger centre, clamped inside the bubble.
    expect(tooltipState.caretLeft).toBeGreaterThanOrEqual(12);
    expect(tooltipState.caretLeft).toBeLessThanOrEqual(120 - 12);
  });

  it('a hide from a different owner does not close a tooltip owned by another trigger', () => {
    const a = {};
    const b = {};
    showTooltip(rect, 'Hello', { placement: 'top', immediate: true, owner: a });
    expect(tooltipState.visible).toBe(true);
    // b never owned this tooltip — its hide is ignored.
    hideTooltip(b);
    expect(tooltipState.visible).toBe(true);
    // the real owner hides it; an argument-less hide (global dismiss) would too.
    hideTooltip(a);
    expect(tooltipState.visible).toBe(false);
  });
});
