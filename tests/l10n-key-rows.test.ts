import { describe, expect, it } from 'vitest';
import type { KeyRow } from '$lib/ipc/bindings';
import { countKeyStates, displayValue, filterRows } from '$lib/l10n/key-rows';

function row(over: Partial<KeyRow> = {}): KeyRow {
  return {
    key: 'item.create.wrench',
    sourceEn: 'Wrench',
    modValue: null,
    overrideValue: null,
    state: 'missing',
    ...over,
  };
}

describe('displayValue', () => {
  it('prefers the override over the mod value', () => {
    const r = row({ modValue: 'Гаечный ключ', overrideValue: 'Мой ключ', state: 'ok' });
    expect(displayValue(r)).toBe('Мой ключ');
  });

  it('falls back to the mod value when there is no override', () => {
    const r = row({ modValue: 'Гаечный ключ', overrideValue: null, state: 'from_mod' });
    expect(displayValue(r)).toBe('Гаечный ключ');
  });

  it('is empty when neither the mod nor the user has a translation', () => {
    const r = row({ modValue: null, overrideValue: null, state: 'missing' });
    expect(displayValue(r)).toBe('');
  });

  // Orphan: the mod dropped the key, but the user's override is still what
  // would show if it mattered — displayValue doesn't editorialise about
  // whether the value is reachable in game, it just says what's stored.
  it('shows the override for an orphaned key', () => {
    const r = row({ modValue: null, overrideValue: 'Стоп-кран', state: 'orphan' });
    expect(displayValue(r)).toBe('Стоп-кран');
  });
});

describe('filterRows', () => {
  const rows: KeyRow[] = [
    row({ key: 'gui.create.title', sourceEn: 'Create', state: 'from_mod', modValue: 'Крафт' }),
    row({
      key: 'gui.create.wrench',
      sourceEn: 'Wrench',
      state: 'ok',
      overrideValue: 'Ключ',
    }),
    row({
      key: 'gui.create.saw',
      sourceEn: 'Saw',
      state: 'stale',
      overrideValue: 'Пила старая',
      modValue: 'Saw (new)',
    }),
    row({
      key: 'gui.create.ghost',
      sourceEn: 'Ghost item',
      state: 'orphan',
      overrideValue: 'Призрак',
    }),
    row({ key: 'gui.create.untranslated', sourceEn: 'Untranslated thing', state: 'missing' }),
  ];

  it('returns every row for "all" with no search', () => {
    expect(filterRows(rows, '', 'all')).toHaveLength(5);
  });

  it('"translated" includes both ok and from_mod, nothing else', () => {
    const result = filterRows(rows, '', 'translated');
    expect(result.map((r) => r.key).sort()).toEqual(['gui.create.title', 'gui.create.wrench']);
  });

  it('"missing" isolates untranslated keys', () => {
    const result = filterRows(rows, '', 'missing');
    expect(result.map((r) => r.key)).toEqual(['gui.create.untranslated']);
  });

  it('"stale" isolates overrides whose English changed', () => {
    const result = filterRows(rows, '', 'stale');
    expect(result.map((r) => r.key)).toEqual(['gui.create.saw']);
  });

  it('"orphan" isolates overrides for keys the mod no longer ships', () => {
    const result = filterRows(rows, '', 'orphan');
    expect(result.map((r) => r.key)).toEqual(['gui.create.ghost']);
  });

  it('matches the search text against the key', () => {
    const result = filterRows(rows, 'wrench', 'all');
    expect(result.map((r) => r.key)).toEqual(['gui.create.wrench']);
  });

  it('matches the search text against the English source, not just the key', () => {
    const result = filterRows(rows, 'ghost item', 'all');
    expect(result.map((r) => r.key)).toEqual(['gui.create.ghost']);
  });

  it('does not match against the translated value — the user may not be able to read it yet', () => {
    // "Ключ" (Russian for "wrench") is the override value, not the key or the
    // English source; searching for it must not surface the row, otherwise a
    // user who can't read Cyrillic could never find a row by typing English.
    const result = filterRows(rows, 'ключ', 'all');
    expect(result).toHaveLength(0);
  });

  it('is case-insensitive', () => {
    const result = filterRows(rows, 'WRENCH', 'all');
    expect(result.map((r) => r.key)).toEqual(['gui.create.wrench']);
  });

  it('trims whitespace from the search term', () => {
    const result = filterRows(rows, '  wrench  ', 'all');
    expect(result.map((r) => r.key)).toEqual(['gui.create.wrench']);
  });

  it('combines an active filter with a search term', () => {
    const result = filterRows(rows, 'create', 'orphan');
    expect(result.map((r) => r.key)).toEqual(['gui.create.ghost']);
  });

  it('returns nothing when the search matches no row under the active filter', () => {
    const result = filterRows(rows, 'wrench', 'orphan');
    expect(result).toHaveLength(0);
  });
});

describe('countKeyStates', () => {
  it('buckets every row into exactly one state, plus a running "all" total', () => {
    const rows: KeyRow[] = [
      row({ key: 'a', state: 'from_mod' }),
      row({ key: 'b', state: 'ok' }),
      row({ key: 'c', state: 'stale' }),
      row({ key: 'd', state: 'orphan' }),
      row({ key: 'e', state: 'missing' }),
      row({ key: 'f', state: 'missing' }),
    ];
    expect(countKeyStates(rows)).toEqual({
      all: 6,
      translated: 2, // from_mod + ok
      stale: 1,
      orphan: 1,
      missing: 2,
    });
  });

  it('is all-zero for an empty row set', () => {
    expect(countKeyStates([])).toEqual({ all: 0, translated: 0, stale: 0, orphan: 0, missing: 0 });
  });
});
