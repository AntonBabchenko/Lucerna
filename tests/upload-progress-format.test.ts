import { describe, expect, it } from 'vitest';
import {
  advanceProgressDisplay,
  emptyProgressDisplay,
  estimateSpeedBytesPerSec,
  etaSeconds,
  formatEtaClock,
  formatRate,
  formatUploadProgress,
} from '$lib/servers/upload-progress-format';
import { enTr } from './test-utils/en-translator';

describe('estimateSpeedBytesPerSec', () => {
  it('returns 0 for fewer than two samples', () => {
    expect(estimateSpeedBytesPerSec([])).toBe(0);
    expect(estimateSpeedBytesPerSec([{ bytes: 1000, atMs: 0 }])).toBe(0);
  });

  it('computes rate from two samples', () => {
    // 1000 bytes in 1000ms → 1000 B/s
    const samples = [
      { bytes: 0, atMs: 0 },
      { bytes: 1000, atMs: 1000 },
    ];
    expect(estimateSpeedBytesPerSec(samples)).toBe(1000);
  });

  it('applies EWMA smoothing over three samples', () => {
    // interval 1: 1000 B in 1000ms → 1000 B/s (first ewma = 1000)
    // interval 2: 2000 B in 1000ms → 2000 B/s; ewma = 0.3*2000 + 0.7*1000 = 1300
    const samples = [
      { bytes: 0, atMs: 0 },
      { bytes: 1000, atMs: 1000 },
      { bytes: 3000, atMs: 2000 },
    ];
    expect(estimateSpeedBytesPerSec(samples)).toBeCloseTo(1300, 5);
  });

  it('skips intervals with zero or negative time delta', () => {
    // dt=0 between samples 0 and 1 → that interval is skipped.
    // Interval 1→2: db = 1000-500 = 500, dt = 1000ms → 500 B/s; ewma = 500.
    const samples = [
      { bytes: 0, atMs: 0 },
      { bytes: 500, atMs: 0 }, // same timestamp — dt=0, skip
      { bytes: 1000, atMs: 1000 },
    ];
    expect(estimateSpeedBytesPerSec(samples)).toBe(500);
  });

  it('returns 0 if all intervals have non-positive dt', () => {
    const samples = [
      { bytes: 0, atMs: 1000 },
      { bytes: 500, atMs: 1000 },
    ];
    expect(estimateSpeedBytesPerSec(samples)).toBe(0);
  });
});

describe('etaSeconds', () => {
  it('returns null when speed is zero', () => {
    expect(etaSeconds(1000, 0, 0)).toBeNull();
  });

  it('returns null when speed is negative', () => {
    expect(etaSeconds(1000, 0, -5)).toBeNull();
  });

  it('returns 0 when already complete (bytesDone >= bytesTotal)', () => {
    expect(etaSeconds(1000, 1000, 500)).toBe(0);
    expect(etaSeconds(1000, 1500, 500)).toBe(0);
  });

  it('computes remaining time correctly', () => {
    // 500 bytes remaining at 100 B/s → 5 seconds
    expect(etaSeconds(1000, 500, 100)).toBeCloseTo(5, 5);
  });
});

describe('formatEtaClock', () => {
  it('renders --:-- for null', () => {
    expect(formatEtaClock(null)).toBe('--:--');
  });

  it('renders --:-- for non-finite values', () => {
    expect(formatEtaClock(Infinity)).toBe('--:--');
    expect(formatEtaClock(NaN)).toBe('--:--');
  });

  it('pads seconds to two digits', () => {
    expect(formatEtaClock(65)).toBe('1:05');
    expect(formatEtaClock(9)).toBe('0:09');
  });

  it('renders whole minutes', () => {
    expect(formatEtaClock(120)).toBe('2:00');
  });

  it('clamps negative to 0:00', () => {
    expect(formatEtaClock(-10)).toBe('0:00');
  });
});

describe('formatRate', () => {
  it('renders bytes/s for sub-KB rates', () => {
    expect(formatRate(enTr, 512)).toBe('512 B/s');
  });

  it('renders KB/s for KB-range rates', () => {
    // 1536 B/s = 1.5 KB/s
    expect(formatRate(enTr, 1536)).toBe('1.5 KB/s');
  });

  it('renders MB/s for MB-range rates', () => {
    // 1.5 * 1024 * 1024 B/s = 1.5 MB/s
    expect(formatRate(enTr, 1.5 * 1024 * 1024)).toBe('1.5 MB/s');
  });

  it('clamps negative to 0 B/s', () => {
    expect(formatRate(enTr, -100)).toBe('0 B/s');
  });
});

describe('formatUploadProgress', () => {
  it('builds the full progress line with MB sizes and speed', () => {
    // 1 MiB done, 4 MiB total, 1 MiB/s → ETA ~3s
    const MB = 1024 * 1024;
    const result = formatUploadProgress(enTr, {
      bytesDone: MB,
      bytesTotal: 4 * MB,
      speedBytesPerSec: MB,
      etaSecondsValue: 3,
    });
    // Must contain MB sizes
    expect(result).toContain('MB');
    // Must contain the rate and ETA separator
    expect(result).toContain('·');
    // ETA 3s → 0:03
    expect(result).toContain('0:03');
  });

  it('renders --:-- placeholder when speed is unknown (etaSecondsValue null)', () => {
    const MB = 1024 * 1024;
    const result = formatUploadProgress(enTr, {
      bytesDone: MB,
      bytesTotal: 4 * MB,
      speedBytesPerSec: 0,
      etaSecondsValue: null,
    });
    expect(result).toContain('--:--');
  });
});

describe('advanceProgressDisplay (1 Hz throttle)', () => {
  const REFRESH = 1000;

  it('emptyProgressDisplay starts at zero/unknown', () => {
    const d = emptyProgressDisplay();
    expect(d.bytesDone).toBe(0);
    expect(d.speedBytesPerSec).toBe(0);
    expect(d.etaSecondsValue).toBeNull();
    expect(d.samples).toEqual([]);
    expect(d.lastRefreshMs).toBe(0);
  });

  it('refreshes immediately on the first call (never-refreshed sentinel)', () => {
    const d = advanceProgressDisplay(emptyProgressDisplay(), 1000, 100_000, 1000, REFRESH);
    expect(d.bytesDone).toBe(1000);
    expect(d.lastRefreshMs).toBe(1000);
    expect(d.samples).toHaveLength(1);
    // One sample → speed not yet estimable.
    expect(d.speedBytesPerSec).toBe(0);
  });

  it('FREEZES the display within the refresh window (same reference, ignores new bytes)', () => {
    const d0 = advanceProgressDisplay(emptyProgressDisplay(), 1000, 100_000, 1000, REFRESH);
    const d1 = advanceProgressDisplay(d0, 5000, 100_000, 1500, REFRESH); // +500ms, within window
    expect(d1).toBe(d0); // unchanged reference — the rendered line cannot flicker
    expect(d1.bytesDone).toBe(1000); // still the frozen value, not 5000
  });

  it('refreshes once the window has elapsed and recomputes a windowed speed', () => {
    const d0 = advanceProgressDisplay(emptyProgressDisplay(), 1000, 100_000, 1000, REFRESH);
    const d2 = advanceProgressDisplay(d0, 5000, 100_000, 2000, REFRESH); // +1000ms → refresh
    expect(d2).not.toBe(d0);
    expect(d2.bytesDone).toBe(5000);
    expect(d2.lastRefreshMs).toBe(2000);
    // (5000-1000) bytes over 1000ms → 4000 B/s.
    expect(d2.speedBytesPerSec).toBe(4000);
    // ETA = remaining / speed = (100000-5000)/4000.
    expect(d2.etaSecondsValue).toBeCloseTo((100_000 - 5000) / 4000, 5);
  });

  it('does not append a duplicate sample when bytes did not advance', () => {
    const a = advanceProgressDisplay(emptyProgressDisplay(), 1000, 100_000, 1000, REFRESH);
    const b = advanceProgressDisplay(a, 1000, 100_000, 2000, REFRESH); // same bytes, after window
    expect(b.samples).toHaveLength(1);
    expect(b.lastRefreshMs).toBe(2000);
  });

  it('caps the sample buffer at 30 across a long upload', () => {
    let d = emptyProgressDisplay();
    for (let i = 0; i < 40; i++) {
      const t = (i + 1) * 1000; // avoid the t=0 "never refreshed" sentinel
      d = advanceProgressDisplay(d, i * 10_000, 1_000_000_000, t, REFRESH);
    }
    expect(d.samples.length).toBe(30);
  });
});
