import { get } from 'svelte/store';
import { afterEach, describe, expect, it } from 'vitest';
import { locale, t } from '$lib/i18n';

// The download counters are handed the RAW number; the dictionary's
// `{downloads, number}` / `{n, number}` argument owns the grouping, so the
// separator follows the APP locale. A pre-formatted `.toLocaleString()` string
// followed the OS locale instead — an English UI on a Russian Windows printed
// "12 345". These go through the real svelte-i18n store rather than `enTr`,
// which does naive `{placeholder}` substitution and cannot render an ICU
// number argument — same harness and same reason as tests/format/size.test.ts.

afterEach(() => locale.set('en'));

// Intl renders the Russian group separator as a NO-BREAK SPACE. Normalising
// every Unicode space to a plain one keeps the assertions readable and
// survives a CLDR bump that swaps U+00A0 for U+202F, while still failing if
// the separator vanishes or turns into a comma. `\s` rather than a literal
// character class on purpose: U+00A0 and U+202F are invisible in source, and
// the JS `\s` class covers both by specification.
const spaces = (s: string) => s.replace(/\s/g, ' ');

describe('mods.card.byAuthorDownloads', () => {
  it('groups the count with the English separator', () => {
    locale.set('en');
    expect(get(t)('mods.card.byAuthorDownloads', { author: 'someone', downloads: 12345 })).toBe(
      'by someone · 12,345 dl',
    );
  });

  it('groups the count with the Russian separator', () => {
    locale.set('ru');
    const text = get(t)('mods.card.byAuthorDownloads', { author: 'someone', downloads: 12345 });
    expect(spaces(text)).toBe('someone · 12 345 скач.');
  });
});

describe('mods.detail.byAuthorSourceDownloads', () => {
  it('groups the count with the English separator', () => {
    locale.set('en');
    expect(
      get(t)('mods.detail.byAuthorSourceDownloads', {
        author: 'someone',
        source: 'modrinth',
        downloads: 12345,
      }),
    ).toBe('by someone · modrinth · 12,345 downloads');
  });

  it('groups the count with the Russian separator', () => {
    locale.set('ru');
    const text = get(t)('mods.detail.byAuthorSourceDownloads', {
      author: 'someone',
      source: 'modrinth',
      downloads: 12345,
    });
    expect(spaces(text)).toBe('someone · modrinth · 12 345 скачиваний');
  });
});

describe('format.count', () => {
  // The label-less sites (ModCard's list row, ServerContentDetail's header)
  // render this alone next to an icon, so the key carries no unit at all.
  it('is a bare grouped integer in English', () => {
    locale.set('en');
    expect(get(t)('format.count', { n: 12345 })).toBe('12,345');
  });

  it('is a bare grouped integer in Russian', () => {
    locale.set('ru');
    expect(spaces(get(t)('format.count', { n: 12345 }))).toBe('12 345');
  });

  it('renders zero without a separator', () => {
    locale.set('en');
    expect(get(t)('format.count', { n: 0 })).toBe('0');
  });
});
