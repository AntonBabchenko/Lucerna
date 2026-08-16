import { get } from 'svelte/store';
import { afterEach, describe, expect, it } from 'vitest';
import { formatCount } from '$lib/format/count';
import { locale, t } from '$lib/i18n';

// The download counters are handed the RAW number; the dictionary's
// `{downloads, number}` argument owns the grouping, so the separator follows
// the APP locale. A pre-formatted `.toLocaleString()` string followed the OS
// locale instead — an English UI on a Russian Windows printed "12 345". These
// go through the real svelte-i18n store rather than `enTr`, which does naive
// `{placeholder}` substitution and cannot render an ICU number argument — same
// harness and same reason as tests/format/size.test.ts.
//
// The label-less sites use $lib/format/count.ts instead of a key, for a reason
// spelled out in that module and pinned by the last describe below.

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

describe('formatCount', () => {
  // ModCard's list row and ServerContentDetail's header render a bare count
  // next to an icon: no unit, so nothing to translate — only the separator.
  it('is a bare grouped integer in English', () => {
    expect(formatCount('en', 12345)).toBe('12,345');
  });

  it('is a bare grouped integer in Russian', () => {
    expect(spaces(formatCount('ru', 12345))).toBe('12 345');
  });

  it('does not let the first locale it rendered bind every later one', () => {
    // The regression that moved this out of the dictionary. As a key, its en
    // and ru values were byte-identical ("{n, number}"), and svelte-i18n
    // memoizes message formatters on the message STRING alone — so whichever
    // locale rendered first won for the whole process and Russian kept
    // printing "12,345". Both directions are asserted in one test on purpose:
    // the bug was only visible in the SECOND call.
    expect(formatCount('en', 12345)).toBe('12,345');
    expect(spaces(formatCount('ru', 12345))).toBe('12 345');
    expect(formatCount('en', 12345)).toBe('12,345');
  });

  it('renders zero without a separator', () => {
    expect(formatCount('en', 0)).toBe('0');
  });

  it('falls back to the host default before the locale store resolves', () => {
    // null is what svelte-i18n yields until initLocale() reconciles the
    // persisted preference — it must not throw or render an empty string.
    expect(formatCount(null, 12345)).not.toBe('');
    expect(formatCount(undefined, 12345)).not.toBe('');
  });
});
