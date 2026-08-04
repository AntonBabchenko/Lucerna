import { describe, expect, it } from 'vitest';
import { advanceProgressDisplay, emptyProgressDisplay } from '$lib/servers/upload-progress-format';
import { canShowRate, toTaskRate } from '$lib/tasks/rate';

describe('canShowRate', () => {
  it('allows a byte phase with a known total', () => {
    expect(canShowRate({ current: 10, total: 100, unit: 'bytes' })).toBe(true);
  });

  it('refuses a file phase', () => {
    // 42 of 120 library files says nothing about transfer rate.
    expect(canShowRate({ current: 42, total: 120, unit: 'files' })).toBe(false);
  });

  it('refuses a byte phase with no known total', () => {
    // No Content-Length: percentage and ETA are both undefined.
    expect(canShowRate({ current: 10, total: 0, unit: 'bytes' })).toBe(false);
  });

  it('refuses a null progress', () => {
    expect(canShowRate(null)).toBe(false);
  });
});

describe('toTaskRate', () => {
  it('renames the display fields to the task registry shape', () => {
    const display = advanceProgressDisplay(emptyProgressDisplay(), 1000, 100_000, 1000, 1000);
    expect(toTaskRate(display)).toEqual({
      bytesPerSec: display.speedBytesPerSec,
      etaSeconds: display.etaSecondsValue,
    });
  });

  it('carries a null ETA through unchanged when speed is not yet estimable', () => {
    const display = emptyProgressDisplay();
    expect(toTaskRate(display)).toEqual({ bytesPerSec: 0, etaSeconds: null });
  });
});
