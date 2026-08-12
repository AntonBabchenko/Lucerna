import { describe, expect, it } from 'vitest';
import { normalize, searchSettings } from '$lib/settings/search';
import type { SettingsSearchEntry } from '$lib/settings/search-index';

const ENTRIES: SettingsSearchEntry[] = [
  { anchor: 'game.gpu', tab: 'game', labelKey: 'L.gpu' as never, keywordsKey: 'K.gpu' as never },
  {
    anchor: 'appearance.theme',
    tab: 'appearance',
    labelKey: 'L.theme' as never,
    keywordsKey: 'K.theme' as never,
  },
];

const STRINGS: Record<string, string> = {
  'L.gpu': 'Preferred GPU',
  'K.gpu': 'gpu graphics видеокарта',
  'L.theme': 'Theme',
  'K.theme': 'theme тёмная',
  'settings.sections.game': 'Game',
  'settings.sections.appearance': 'Appearance',
};
const t = (k: string): string => STRINGS[k] ?? k;

describe('normalize', () => {
  it('lowercases, trims, and strips diacritics (incl. Cyrillic ё → е)', () => {
    expect(normalize('  Тёмная ')).toBe('темная');
    expect(normalize('GPU')).toBe('gpu');
  });
});

describe('searchSettings', () => {
  it('returns [] for an empty or whitespace query', () => {
    expect(searchSettings('', ENTRIES, t as never)).toEqual([]);
    expect(searchSettings('   ', ENTRIES, t as never)).toEqual([]);
  });

  it('matches an English keyword case-insensitively', () => {
    expect(searchSettings('GRAPHICS', ENTRIES, t as never).map((e) => e.anchor)).toEqual([
      'game.gpu',
    ]);
  });

  it('matches a Russian keyword ignoring diacritics (темная → тёмная)', () => {
    expect(searchSettings('темная', ENTRIES, t as never).map((e) => e.anchor)).toEqual([
      'appearance.theme',
    ]);
  });

  it('requires every whitespace token to match (AND semantics)', () => {
    expect(searchSettings('preferred gpu', ENTRIES, t as never).map((e) => e.anchor)).toEqual([
      'game.gpu',
    ]);
    expect(searchSettings('preferred theme', ENTRIES, t as never)).toEqual([]);
  });

  it('also matches on the section name', () => {
    expect(searchSettings('appearance', ENTRIES, t as never).map((e) => e.anchor)).toEqual([
      'appearance.theme',
    ]);
  });

  it('returns [] when nothing matches', () => {
    expect(searchSettings('zzzz', ENTRIES, t as never)).toEqual([]);
  });
});
