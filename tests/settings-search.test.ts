import { describe, expect, it } from 'vitest';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import en from '$lib/i18n/locales/en.json';
import ru from '$lib/i18n/locales/ru.json';
import { normalize, searchSettings } from '$lib/settings/search';
import type { SettingsSearchEntry } from '$lib/settings/search-index';
import { SETTINGS_ENTRIES } from '$lib/settings/search-index';

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

function localeT(dict: unknown): (key: TranslationKey) => string {
  return (key) =>
    String(
      key.split('.').reduce<unknown>((o, part) => {
        if (o && typeof o === 'object') return (o as Record<string, unknown>)[part];
        return undefined;
      }, dict) ?? '',
    );
}

describe('searchSettings over the real registry + locales', () => {
  const enT = localeT(en);
  const ruT = localeT(ru);
  const first = (query: string, t: (key: TranslationKey) => string) =>
    searchSettings(query, SETTINGS_ENTRIES, t)[0]?.anchor;

  it('routes common EN queries to the right top result', () => {
    expect(first('gpu', enT)).toBe('game.gpu');
    expect(first('language', enT)).toBe('appearance.language');
    expect(first('data', enT)).toBe('storage.dataLocation');
  });

  it('ranks the label match above a buried keyword substring for "mod"', () => {
    expect(first('mod', enT)).toBe('storage.modMetadataCache');
    expect(first('мод', ruT)).toBe('storage.modMetadataCache');
  });

  it('routes Russian queries by their own-language labels/synonyms', () => {
    expect(first('тема', ruT)).toBe('appearance.theme');
    expect(first('язык', ruT)).toBe('appearance.language');
    expect(first('видеокарта', ruT)).toBe('game.gpu');
  });
});
