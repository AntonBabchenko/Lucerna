import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { relativeDate, relativeTime } from '$lib/format/relative-time';
import { enTr } from './test-utils/en-translator';

const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

// Pin Date.now() so relative calculations are deterministic.
const NOW = new Date('2024-01-15T12:00:00Z').getTime();

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(NOW);
});

afterEach(() => {
  vi.useRealTimers();
});

// ── relativeTime ─────────────────────────────────────────────────────────────

describe('relativeTime', () => {
  it('returns "never" for null', () => {
    expect(relativeTime(enTr, null)).toBe('never');
  });

  it('returns "never" for undefined', () => {
    expect(relativeTime(enTr, undefined)).toBe('never');
  });

  it('returns "never" for 0', () => {
    expect(relativeTime(enTr, 0)).toBe('never');
  });

  it('renders seconds ago for diff < 60s', () => {
    expect(relativeTime(enTr, NOW - 30 * SECOND)).toBe('30s ago');
    expect(relativeTime(enTr, NOW - 1 * SECOND)).toBe('1s ago');
  });

  it('renders minutes ago for diff in [1m, 1h)', () => {
    expect(relativeTime(enTr, NOW - 1 * MINUTE)).toBe('1m ago');
    expect(relativeTime(enTr, NOW - 45 * MINUTE)).toBe('45m ago');
  });

  it('renders hours ago for diff in [1h, 24h)', () => {
    expect(relativeTime(enTr, NOW - 1 * HOUR)).toBe('1h ago');
    expect(relativeTime(enTr, NOW - 23 * HOUR)).toBe('23h ago');
  });

  it('renders days ago for diff >= 24h', () => {
    expect(relativeTime(enTr, NOW - 1 * DAY)).toBe('1d ago');
    expect(relativeTime(enTr, NOW - 7 * DAY)).toBe('7d ago');
  });
});

// ── relativeDate ─────────────────────────────────────────────────────────────

describe('relativeDate', () => {
  it('returns "" for null', () => {
    expect(relativeDate(enTr, null)).toBe('');
  });

  it('returns "today" for a timestamp within the past day', () => {
    expect(relativeDate(enTr, NOW - 1 * HOUR)).toBe('today');
    expect(relativeDate(enTr, NOW - 23 * HOUR)).toBe('today');
  });

  it('returns "yesterday" for a timestamp in [1d, 2d)', () => {
    expect(relativeDate(enTr, NOW - 1 * DAY - 1 * HOUR)).toBe('yesterday');
    expect(relativeDate(enTr, NOW - 2 * DAY + 1 * MINUTE)).toBe('yesterday');
  });

  it('returns "Nd ago" for a timestamp in [2d, 30d)', () => {
    expect(relativeDate(enTr, NOW - 2 * DAY)).toBe('2d ago');
    expect(relativeDate(enTr, NOW - 15 * DAY)).toBe('15d ago');
  });

  it('returns "Nmo ago" for a timestamp in [30d, 12mo)', () => {
    expect(relativeDate(enTr, NOW - 30 * DAY)).toBe('1mo ago');
    expect(relativeDate(enTr, NOW - 90 * DAY)).toBe('3mo ago');
  });

  it('returns "Ny ago" for a timestamp >= 12 months', () => {
    expect(relativeDate(enTr, NOW - 365 * DAY)).toBe('1y ago');
    expect(relativeDate(enTr, NOW - 730 * DAY)).toBe('2y ago');
  });
});
