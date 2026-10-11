import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { getOsLang, matchSystemLocale, pickerValue, resolveLocale } from '../src/lib/i18n/resolve';

const AVAILABLE = ['en', 'ru'];
const SYSTEM = ['en', 'ru'];

describe('resolveLocale', () => {
  it('returns an explicit pref when it is available', () => {
    expect(resolveLocale('ru', AVAILABLE, 'en-US', SYSTEM)).toBe('ru');
    expect(resolveLocale('en', AVAILABLE, 'ru-RU', SYSTEM)).toBe('en');
  });

  it('falls back to system handling when the explicit pref was removed', () => {
    // 'de' no longer shipped → behave as if system, OS is ru → ru
    expect(resolveLocale('de', AVAILABLE, 'ru-RU', SYSTEM)).toBe('ru');
    // 'de' removed, OS is English → en
    expect(resolveLocale('de', AVAILABLE, 'en-GB', SYSTEM)).toBe('en');
  });

  it("system resolves to ru when OS language starts with 'ru' and ru is available", () => {
    expect(resolveLocale('system', AVAILABLE, 'ru-RU', SYSTEM)).toBe('ru');
    expect(resolveLocale('system', AVAILABLE, 'ru', SYSTEM)).toBe('ru');
  });

  it('system resolves to en when the OS language has no translation', () => {
    expect(resolveLocale('system', AVAILABLE, 'en-US', SYSTEM)).toBe('en');
    expect(resolveLocale('system', AVAILABLE, 'fr-FR', SYSTEM)).toBe('en');
    expect(resolveLocale('system', AVAILABLE, '', SYSTEM)).toBe('en');
  });

  it('system resolves to en when ru is not available even if OS is ru', () => {
    expect(resolveLocale('system', ['en'], 'ru-RU', ['en'])).toBe('en');
  });

  it('system picks any shipped language, not only Russian', () => {
    const avail = ['de', 'en', 'pt-BR', 'ru', 'zh-Hans'];
    expect(resolveLocale('system', avail, 'de-AT', avail)).toBe('de');
    expect(resolveLocale('system', avail, 'pt-BR', avail)).toBe('pt-BR');
    expect(resolveLocale('system', avail, 'zh-CN', avail)).toBe('zh-Hans');
  });

  it('system skips a language below the threshold, but an explicit pick of it holds', () => {
    const avail = ['de', 'en', 'ru'];
    const eligible = ['en', 'ru'];
    expect(resolveLocale('system', avail, 'de-DE', eligible)).toBe('en');
    expect(resolveLocale('de', avail, 'ru-RU', eligible)).toBe('de');
  });

  it('a removed explicit pref resolves through the eligible list only', () => {
    expect(resolveLocale('fr', ['de', 'en'], 'de-DE', ['en'])).toBe('en');
    expect(resolveLocale('fr', ['de', 'en'], 'de-DE', ['de', 'en'])).toBe('de');
  });

  it('never resolves to an eligible code that is not shipped', () => {
    expect(resolveLocale('system', ['en'], 'de-DE', ['de', 'en'])).toBe('en');
  });
});

describe('matchSystemLocale', () => {
  it('prefers the exact tag', () => {
    expect(matchSystemLocale('pt-BR', ['pt', 'pt-BR', 'pt-PT'])).toBe('pt-BR');
    expect(matchSystemLocale('de-CH', ['de', 'de-CH'])).toBe('de-CH');
  });

  it('matches through likely subtags: a region-less tag and its default region are one locale', () => {
    expect(matchSystemLocale('de-DE', ['de', 'en'])).toBe('de');
    expect(matchSystemLocale('zh-CN', ['zh-Hans', 'zh-Hant'])).toBe('zh-Hans');
    expect(matchSystemLocale('zh-TW', ['zh-Hans', 'zh-Hant'])).toBe('zh-Hant');
  });

  it('crosses regions within one language and script', () => {
    expect(matchSystemLocale('de-AT', ['de', 'en'])).toBe('de');
    expect(matchSystemLocale('pt-PT', ['en', 'pt-BR'])).toBe('pt-BR');
    expect(matchSystemLocale('en-GB', ['en', 'ru'])).toBe('en');
  });

  it('never crosses scripts', () => {
    expect(matchSystemLocale('zh-TW', ['en', 'zh-Hans'])).toBeNull();
    expect(matchSystemLocale('sr', ['sr-Latn'])).toBeNull();
  });

  it('prefers the region-less candidate, then the first by code', () => {
    expect(matchSystemLocale('de-AT', ['de-CH', 'de'])).toBe('de');
    expect(matchSystemLocale('de-AT', ['de-DE', 'de-CH'])).toBe('de-CH');
  });

  it('accepts a POSIX-spelled OS language and gives up on an unreadable one', () => {
    expect(matchSystemLocale('pt_BR', ['pt-BR'])).toBe('pt-BR');
    expect(matchSystemLocale('', ['en'])).toBeNull();
    expect(matchSystemLocale('not a tag!', ['en'])).toBeNull();
  });
});

describe('matchSystemLocale without Intl.Locale (system WebKit before Safari 14)', () => {
  const realLocale = Intl.Locale;
  beforeEach(() => {
    // biome-ignore lint/suspicious/noExplicitAny: removing a built-in to emulate an old engine
    (Intl as any).Locale = undefined;
  });
  afterEach(() => {
    // biome-ignore lint/suspicious/noExplicitAny: restoring the built-in
    (Intl as any).Locale = realLocale;
  });

  it('still matches a regional OS language to its language', () => {
    expect(matchSystemLocale('ru-RU', ['en', 'ru'])).toBe('ru');
    expect(matchSystemLocale('de-AT', ['de', 'en'])).toBe('de');
    expect(matchSystemLocale('pt-BR', ['en', 'pt-BR'])).toBe('pt-BR');
  });

  it('without likely subtags, never guesses across a script', () => {
    expect(matchSystemLocale('zh-TW', ['en', 'zh-Hans'])).toBeNull();
    expect(matchSystemLocale('zh-CN', ['en', 'zh-Hans'])).toBeNull();
  });

  it('still gives up on an unreadable OS language', () => {
    expect(matchSystemLocale('not a tag!', ['en'])).toBeNull();
  });
});

describe('getOsLang', () => {
  const real = navigator.language;
  afterEach(() => {
    Object.defineProperty(navigator, 'language', { value: real, configurable: true });
  });

  it('reports the OS language as the webview gives it', () => {
    Object.defineProperty(navigator, 'language', { value: 'de-AT', configurable: true });
    expect(getOsLang()).toBe('de-AT');
  });

  it('treats a value that is no language tag as unreadable, like a missing one', () => {
    Object.defineProperty(navigator, 'language', { value: 'not a tag!', configurable: true });
    expect(getOsLang()).toBe('');
  });
});

describe('pickerValue', () => {
  it('shows a shipped pref and System as they are', () => {
    expect(pickerValue('ru', AVAILABLE)).toBe('ru');
    expect(pickerValue('system', AVAILABLE)).toBe('system');
  });

  it('shows System for a pref whose language is no longer shipped', () => {
    expect(pickerValue('de', AVAILABLE)).toBe('system');
  });
});
