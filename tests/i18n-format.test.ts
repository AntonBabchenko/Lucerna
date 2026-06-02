import { get } from 'svelte/store';
import { describe, expect, it } from 'vitest';
import { AVAILABLE_LOCALES, locale, t } from '../src/lib/i18n';

describe('i18n formatting', () => {
  it('discovers en and ru', () => {
    expect(AVAILABLE_LOCALES).toEqual(['en', 'ru']);
  });

  it('formats a key in English then Russian after locale.set', async () => {
    locale.set('en');
    expect(get(t)('common.cancel')).toBe('Cancel');
    locale.set('ru');
    expect(get(t)('common.cancel')).toBe('Отмена');
  });
});
