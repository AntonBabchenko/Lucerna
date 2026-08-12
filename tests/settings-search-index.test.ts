import { describe, expect, it } from 'vitest';
import en from '$lib/i18n/locales/en.json';
import ru from '$lib/i18n/locales/ru.json';
import { SETTINGS_ENTRIES, SETTINGS_SEARCH, shouldFocusAnchor } from '$lib/settings/search-index';

describe('shouldFocusAnchor', () => {
  it('does not grab focus for a select-style anchor', () => {
    expect(shouldFocusAnchor('game.gpu')).toBe(false);
  });

  it('does not grab focus for a toggle-style anchor', () => {
    expect(shouldFocusAnchor('game.tray')).toBe(false);
  });
});

function resolve(dict: unknown, key: string): unknown {
  return key.split('.').reduce<unknown>((o, part) => {
    if (o && typeof o === 'object') return (o as Record<string, unknown>)[part];
    return undefined;
  }, dict);
}

const VALID_TABS = new Set([
  'appearance',
  'game',
  'integrations',
  'storage',
  'updates',
  'help',
  'about',
]);

describe('SETTINGS_SEARCH registry integrity', () => {
  it('every entry resolves to a non-empty label and keywords in en and ru', () => {
    for (const entry of SETTINGS_ENTRIES) {
      for (const dict of [en, ru]) {
        const label = resolve(dict, entry.labelKey);
        const keywords = resolve(dict, entry.keywordsKey);
        expect(typeof label, `${entry.labelKey}`).toBe('string');
        expect((label as string).length, `${entry.labelKey}`).toBeGreaterThan(0);
        expect(typeof keywords, `${entry.keywordsKey}`).toBe('string');
        expect((keywords as string).length, `${entry.keywordsKey}`).toBeGreaterThan(0);
      }
    }
  });

  it('every entry has a valid tab and its Record key equals its anchor', () => {
    for (const [key, entry] of Object.entries(SETTINGS_SEARCH)) {
      expect(entry.anchor).toBe(key);
      expect(VALID_TABS.has(entry.tab)).toBe(true);
    }
  });

  it('SETTINGS_ENTRIES is the Record values in registry order', () => {
    expect(SETTINGS_ENTRIES.length).toBe(Object.keys(SETTINGS_SEARCH).length);
    expect(SETTINGS_ENTRIES[0].anchor).toBe('appearance.theme');
  });
});
