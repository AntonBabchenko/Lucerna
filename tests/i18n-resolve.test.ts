import { describe, expect, it } from 'vitest';
import { resolveLocale } from '../src/lib/i18n/resolve';

const AVAILABLE = ['en', 'ru'];

describe('resolveLocale', () => {
  it('returns an explicit pref when it is available', () => {
    expect(resolveLocale('ru', AVAILABLE, 'en-US')).toBe('ru');
    expect(resolveLocale('en', AVAILABLE, 'ru-RU')).toBe('en');
  });

  it('falls back to system handling when the explicit pref was removed', () => {
    // 'de' no longer shipped → behave as if system, OS is ru → ru
    expect(resolveLocale('de', AVAILABLE, 'ru-RU')).toBe('ru');
    // 'de' removed, OS is English → en
    expect(resolveLocale('de', AVAILABLE, 'en-GB')).toBe('en');
  });

  it("system resolves to ru when OS language starts with 'ru' and ru is available", () => {
    expect(resolveLocale('system', AVAILABLE, 'ru-RU')).toBe('ru');
    expect(resolveLocale('system', AVAILABLE, 'ru')).toBe('ru');
  });

  it('system resolves to en for any non-ru OS language', () => {
    expect(resolveLocale('system', AVAILABLE, 'en-US')).toBe('en');
    expect(resolveLocale('system', AVAILABLE, 'fr-FR')).toBe('en');
    expect(resolveLocale('system', AVAILABLE, '')).toBe('en');
  });

  it('system resolves to en when ru is not available even if OS is ru', () => {
    expect(resolveLocale('system', ['en'], 'ru-RU')).toBe('en');
  });
});
