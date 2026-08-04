import { get } from 'svelte/store';
import { afterEach, describe, expect, it } from 'vitest';
import { formatSize } from '$lib/format/size';
import { locale, t } from '$lib/i18n';

// formatSize hands `t()` the RAW number and lets the dictionary's
// `{n, number, ...}` ICU argument own the rounding AND the decimal
// separator. enTr (tests/test-utils/en-translator.ts) does naive
// `{placeholder}` substitution and does not understand ICU number
// skeletons, so these go through the real svelte-i18n store instead —
// same pattern as tests/i18n-ru-plural-forms.test.ts.

afterEach(() => locale.set('en'));

describe('formatSize', () => {
  it('returns empty string for null, 0, or negative', () => {
    const tr = get(t);
    expect(formatSize(tr, null)).toBe('');
    expect(formatSize(tr, 0)).toBe('');
    expect(formatSize(tr, -5)).toBe('');
  });

  it('formats bytes under 1 KB as B', () => {
    expect(formatSize(get(t), 512)).toBe('512 B');
  });

  it('formats KB with one decimal', () => {
    expect(formatSize(get(t), 1536)).toBe('1.5 KB');
  });

  it('formats MB with one decimal', () => {
    expect(formatSize(get(t), 261361205)).toBe('249.3 MB');
  });

  it('formats the byte below the KB→MB threshold as KB', () => {
    expect(formatSize(get(t), 1048575)).toBe('1024.0 KB');
  });

  it('formats the KB→MB threshold as MB', () => {
    expect(formatSize(get(t), 1048576)).toBe('1.0 MB');
  });

  it('formats GB with two decimals', () => {
    expect(formatSize(get(t), 2684354560)).toBe('2.50 GB');
  });

  describe('Russian locale renders a decimal comma, not a dot', () => {
    it('formats KB with a comma', () => {
      locale.set('ru');
      expect(formatSize(get(t), 1536)).toBe('1,5 КБ');
    });

    it('formats MB with a comma', () => {
      locale.set('ru');
      expect(formatSize(get(t), 261361205)).toBe('249,3 МБ');
    });

    it('formats GB with a comma', () => {
      locale.set('ru');
      expect(formatSize(get(t), 2684354560)).toBe('2,50 ГБ');
    });
  });
});

describe('format.size.perSecond', () => {
  // The upcoming speed readout reuses formatSize() plus this suffix key
  // rather than a second formatter — see src/lib/format/size.ts.
  it('appends the per-second suffix to an already-formatted size (en)', () => {
    const size = formatSize(get(t), 1536);
    expect(get(t)('format.size.perSecond', { size })).toBe('1.5 KB/s');
  });

  it('appends the per-second suffix to an already-formatted size (ru)', () => {
    locale.set('ru');
    const size = formatSize(get(t), 1536);
    expect(get(t)('format.size.perSecond', { size })).toBe('1,5 КБ/с');
  });
});
