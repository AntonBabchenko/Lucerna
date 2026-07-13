// tests/log-hint-hover.test.ts
// The hover state machine behind the inline log-hint card: delayed open,
// hover-bridge grace close, instant retarget/focus, and cleanup. Fake
// timers throughout — mirrors tests/tooltip/tooltip-controller.test.ts.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  type ActiveHint,
  createLogHintHover,
  GRACE_MS,
  type LogHintHover,
  OPEN_DELAY_MS,
} from '$lib/logs/log-hints.svelte';

function mkHint(id: string): ActiveHint {
  return {
    patternId: id,
    copy: { title: `t:${id}`, explanation: 'e', recommendation: 'r' },
    anchor: { top: 100, left: 100, width: 40, height: 20, bottom: 120 },
  };
}

describe('log hint hover state machine', () => {
  let hover: LogHintHover;

  beforeEach(() => {
    vi.useFakeTimers();
    hover = createLogHintHover();
  });

  afterEach(() => {
    hover.dispose();
    vi.useRealTimers();
  });

  it('rowEnter opens only after the open delay', () => {
    hover.rowEnter(mkHint('oom-gc-overhead'));
    vi.advanceTimersByTime(OPEN_DELAY_MS - 1);
    expect(hover.active).toBeNull();
    vi.advanceTimersByTime(1);
    expect(hover.active?.patternId).toBe('oom-gc-overhead');
  });

  it('rowLeave before the delay elapses cancels the pending open', () => {
    hover.rowEnter(mkHint('oom-gc-overhead'));
    hover.rowLeave();
    vi.advanceTimersByTime(OPEN_DELAY_MS + GRACE_MS);
    expect(hover.active).toBeNull();
  });

  it('rowLeave from an open card closes after the grace period', () => {
    hover.openFromFocus(mkHint('oom-gc-overhead'));
    hover.rowLeave();
    vi.advanceTimersByTime(GRACE_MS - 1);
    expect(hover.active?.patternId).toBe('oom-gc-overhead');
    vi.advanceTimersByTime(1);
    expect(hover.active).toBeNull();
  });

  it('cardEnter within the grace keeps the card open (hover bridge); cardLeave closes it', () => {
    hover.openFromFocus(mkHint('oom-gc-overhead'));
    hover.rowLeave();
    vi.advanceTimersByTime(GRACE_MS - 1);
    hover.cardEnter();
    vi.advanceTimersByTime(GRACE_MS * 2);
    expect(hover.active?.patternId).toBe('oom-gc-overhead');
    hover.cardLeave();
    vi.advanceTimersByTime(GRACE_MS);
    expect(hover.active).toBeNull();
  });

  it('rowEnter while open retargets instantly, without the open delay', () => {
    hover.openFromFocus(mkHint('oom-gc-overhead'));
    hover.rowEnter(mkHint('dns-unknown-host'));
    expect(hover.active?.patternId).toBe('dns-unknown-host');
  });

  it('openFromFocus opens instantly (keyboard path)', () => {
    hover.openFromFocus(mkHint('oom-gc-overhead'));
    expect(hover.active?.patternId).toBe('oom-gc-overhead');
  });

  it('close() clears an open card and a pending grace close', () => {
    hover.openFromFocus(mkHint('oom-gc-overhead'));
    hover.rowLeave();
    hover.close();
    expect(hover.active).toBeNull();
    vi.advanceTimersByTime(GRACE_MS + OPEN_DELAY_MS);
    expect(hover.active).toBeNull();
  });

  it('dispose() cancels a pending open timer — nothing opens afterwards', () => {
    hover.rowEnter(mkHint('oom-gc-overhead'));
    hover.dispose();
    vi.advanceTimersByTime(OPEN_DELAY_MS + GRACE_MS);
    expect(hover.active).toBeNull();
  });
});
