import { describe, expect, test } from 'vitest';
import { formatHeapLabel, isAboveRecommended } from '$lib/instances/heap';

describe('formatHeapLabel', () => {
  test('shows GB with one decimal at or above 1 GB', () => {
    expect(formatHeapLabel(8192)).toBe('8.0 GB');
    expect(formatHeapLabel(6144)).toBe('6.0 GB');
    expect(formatHeapLabel(1536)).toBe('1.5 GB');
  });

  test('shows MB below 1 GB', () => {
    expect(formatHeapLabel(768)).toBe('768 MB');
    expect(formatHeapLabel(1023)).toBe('1023 MB');
  });
});

describe('isAboveRecommended', () => {
  test('true only when RAM is known and value exceeds the threshold', () => {
    expect(isAboveRecommended(7000, 6144, true)).toBe(true);
    expect(isAboveRecommended(6144, 6144, true)).toBe(false); // at threshold, not above
    expect(isAboveRecommended(4096, 6144, true)).toBe(false);
  });

  test('never warns when RAM is unknown', () => {
    expect(isAboveRecommended(99999, 8192, false)).toBe(false);
  });
});
