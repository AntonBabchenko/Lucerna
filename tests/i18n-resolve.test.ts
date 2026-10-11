import { describe, expect, it } from 'vitest';
import { matchSystemLocale, pickerValue, resolveLocale } from '../src/lib/i18n/resolve';

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

describe('pickerValue', () => {
  it('shows a shipped pref and System as they are', () => {
    expect(pickerValue('ru', AVAILABLE)).toBe('ru');
    expect(pickerValue('system', AVAILABLE)).toBe('system');
  });

  it('shows System for a pref whose language is no longer shipped', () => {
    expect(pickerValue('de', AVAILABLE)).toBe('system');
  });
});
