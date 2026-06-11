import { describe, expect, it } from 'vitest';
import { formatDuration } from '$lib/format/duration';
import { enTr } from './test-utils/en-translator';

describe('formatDuration', () => {
  it('renders < 1m for sub-minute values', () => {
    expect(formatDuration(enTr, 0)).toBe('< 1m');
    expect(formatDuration(enTr, 59)).toBe('< 1m');
  });

  it('renders minutes-only when under one hour', () => {
    expect(formatDuration(enTr, 60)).toBe('1m');
    expect(formatDuration(enTr, 120)).toBe('2m');
    expect(formatDuration(enTr, 59 * 60)).toBe('59m');
  });

  it('renders Xh Ym when at least one hour', () => {
    expect(formatDuration(enTr, 60 * 60)).toBe('1h 0m');
    expect(formatDuration(enTr, 60 * 60 + 30 * 60)).toBe('1h 30m');
    expect(formatDuration(enTr, 12 * 60 * 60 + 30 * 60)).toBe('12h 30m');
  });

  it('coerces null and undefined to 0 (renders < 1m)', () => {
    expect(formatDuration(enTr, null)).toBe('< 1m');
    expect(formatDuration(enTr, undefined)).toBe('< 1m');
  });
});
