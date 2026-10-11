import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { get } from 'svelte/store';
import { addMessages } from 'svelte-i18n';
import { describe, expect, it, vi } from 'vitest';
import { AVAILABLE_LOCALES, locale, t } from '../src/lib/i18n';

describe('i18n formatting', () => {
  it('discovers en and ru, sorted, among whatever else ships', () => {
    // Not an exact list: a community language arrives as a file, with no
    // registration and no test to update.
    expect(AVAILABLE_LOCALES).toEqual(expect.arrayContaining(['en', 'ru']));
    expect([...AVAILABLE_LOCALES]).toEqual([...AVAILABLE_LOCALES].sort());
  });

  it('formats a key in English then Russian after locale.set', async () => {
    locale.set('en');
    expect(get(t)('common.cancel')).toBe('Cancel');
    locale.set('ru');
    expect(get(t)('common.cancel')).toBe('Отмена');
  });

  // A partial community translation falls back to the English message for
  // every key it lacks, so it shares that message text with English. svelte-i18n
  // memoizes its formatter on the message text alone and fixes the locale at
  // the first call: a number first formatted in German kept German grouping
  // after a switch to English. The bug only shows on the SECOND locale, so the
  // order matters: partial first, then English.
  //
  // Synthetic on purpose: a test-only key that exists in English alone, and a
  // private-use German locale no real dictionary will ever be named, so the
  // test means the same thing whatever languages ship.
  it('formats a message a partial locale shares with English in each locale’s own way', () => {
    addMessages('en', { __test: { count: '{n, number} things' } });
    addMessages('de-x-partial', {});
    locale.set('de-x-partial');
    expect(get(t)('__test.count' as never, { n: 1234567 })).toBe('1.234.567 things');
    locale.set('en');
    expect(get(t)('__test.count' as never, { n: 1234567 })).toBe('1,234,567 things');
  });

  it('shows a message it cannot format as written, and says so', () => {
    locale.set('en');
    addMessages('en', { __test: { broken: 'Count: {n, number' } });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      expect(get(t)('__test.broken' as never, { n: 1 })).toBe('Count: {n, number');
      expect(warn).toHaveBeenCalledTimes(1);
    } finally {
      warn.mockRestore();
    }
  });

  it('gives a new translator when a dictionary gains messages', () => {
    locale.set('en');
    const seen: string[] = [];
    const stop = t.subscribe((tr) => seen.push(tr('__test.late' as never)));
    addMessages('en', { __test: { late: 'Arrived' } });
    stop();
    expect(seen.at(0)).toBe('__test.late');
    expect(seen.at(-1)).toBe('Arrived');
  });

  it('renders a key with no message anywhere as the key', () => {
    locale.set('en');
    expect(get(t)('no.such.key' as never)).toBe('no.such.key');
    expect(get(t)('no.such.key' as never, { n: 1 })).toBe('no.such.key');
  });
});

function sourceFiles(dir: string, acc: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) sourceFiles(full, acc);
    else if (/\.(svelte|ts)$/.test(entry.name)) acc.push(full);
  }
  return acc;
}

// The fix above lives in $lib/i18n/typed.ts. A component that imported
// svelte-i18n's own `_` / `format` / `t` would format through the shared cache
// again, so svelte-i18n stays behind $lib/i18n.
describe('svelte-i18n stays behind $lib/i18n', () => {
  const I18N_DIR = join('src', 'lib', 'i18n');
  const FROM_SVELTE_I18N = /from\s+['"]svelte-i18n['"]/;

  it('is scanning files that do import it inside $lib/i18n', () => {
    // Guards the guard: a regex that stopped matching would pass vacuously.
    expect(readFileSync(join(I18N_DIR, 'typed.ts'), 'utf8')).toMatch(FROM_SVELTE_I18N);
  });

  it('no module outside src/lib/i18n imports svelte-i18n', () => {
    const offenders = sourceFiles('src')
      .filter((f) => !f.startsWith(I18N_DIR))
      .filter((f) => FROM_SVELTE_I18N.test(readFileSync(f, 'utf8')))
      .sort();
    expect(offenders).toEqual([]);
  });
});
