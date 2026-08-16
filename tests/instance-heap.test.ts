import { get } from 'svelte/store';
import { afterEach, describe, expect, test } from 'vitest';
import { locale, t } from '$lib/i18n';
import { formatHeapLabel, isAboveRecommended } from '$lib/instances/heap';

// formatHeapLabel hands `t()` the RAW number and lets the dictionary's
// `{n, number, …}` argument own the rounding and the decimal separator, so it
// goes through the real svelte-i18n store rather than `enTr`, which does naive
// `{placeholder}` substitution and cannot render an ICU number skeleton — same
// harness and same reason as tests/format/size.test.ts.

afterEach(() => locale.set('en'));

describe('formatHeapLabel', () => {
  test('shows GB with one decimal at or above 1 GB', () => {
    const tr = get(t);
    expect(formatHeapLabel(tr, 8192)).toBe('8.0 GB');
    expect(formatHeapLabel(tr, 6144)).toBe('6.0 GB');
    expect(formatHeapLabel(tr, 1536)).toBe('1.5 GB');
  });

  test('shows whole MB below 1 GB', () => {
    const tr = get(t);
    expect(formatHeapLabel(tr, 768)).toBe('768 MB');
    expect(formatHeapLabel(tr, 1023)).toBe('1023 MB');
  });

  test('groups nothing, so a 4-digit MB count stays readable as a slider label', () => {
    // `::group-off` — "1023 MB", never "1,023 MB".
    expect(formatHeapLabel(get(t), 1023)).not.toContain(',');
  });

  test('translates the unit and the decimal mark in Russian', () => {
    // The label was a hardcoded English template literal; the memory slider
    // read it aloud through aria-valuetext, so a Russian screen reader
    // announced "8.0 GB".
    locale.set('ru');
    const tr = get(t);
    expect(formatHeapLabel(tr, 8192)).toBe('8,0 ГБ');
    expect(formatHeapLabel(tr, 1536)).toBe('1,5 ГБ');
    expect(formatHeapLabel(tr, 768)).toBe('768 МБ');
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
