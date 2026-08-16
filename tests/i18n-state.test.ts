import { get } from 'svelte/store';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const appSettingsGet = vi.fn();
const appSettingsSetGeneral = vi.fn().mockResolvedValue({ status: 'ok', data: null });

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: (...a: unknown[]) => appSettingsGet(...a),
    appSettingsSetGeneral: (...a: unknown[]) => appSettingsSetGeneral(...a),
  },
}));

import { locale } from '../src/lib/i18n';
import { initLocale, langPref, setLocalePref } from '../src/lib/i18n/state.svelte';

beforeEach(() => {
  appSettingsGet.mockReset().mockResolvedValue({
    status: 'ok',
    data: { general: { hide_to_tray_during_game: false, theme: 'system', language: 'system' } },
  });
  appSettingsSetGeneral.mockClear();
  localStorage.clear();
});

describe('i18n state', () => {
  it('initLocale sets pref, resolved locale, and mirrors localStorage', () => {
    initLocale('ru');
    expect(langPref.value).toBe('ru');
    expect(get(locale)).toBe('ru');
    expect(localStorage.getItem('locale')).toBe('ru');
  });

  it('setLocalePref switches the live locale and persists via app_settings_set_general', async () => {
    initLocale('en');
    await setLocalePref('ru');
    expect(langPref.value).toBe('ru');
    expect(get(locale)).toBe('ru');
    expect(localStorage.getItem('locale')).toBe('ru');
    expect(appSettingsSetGeneral).toHaveBeenCalledTimes(1);
    expect(appSettingsSetGeneral.mock.calls[0][0]).toMatchObject({ language: 'ru' });
  });

  it('rolls the locale, the rune and the localStorage mirror back when the read fails', async () => {
    initLocale('en');
    appSettingsGet.mockResolvedValueOnce({ status: 'error', error: { kind: 'io', details: 'x' } });

    await setLocalePref('ru');

    expect(langPref.value).toBe('en');
    expect(get(locale)).toBe('en');
    // The mirror is what the next launch's anti-FOUC path reads; a stale 'ru'
    // there opens the UI in a language app.json does not name.
    expect(localStorage.getItem('locale')).toBe('en');
    expect(appSettingsSetGeneral).not.toHaveBeenCalled();
  });

  it('rolls back when the persist write fails', async () => {
    initLocale('en');
    appSettingsSetGeneral.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', details: 'x' },
    });

    await setLocalePref('ru');

    expect(langPref.value).toBe('en');
    expect(get(locale)).toBe('en');
    expect(localStorage.getItem('locale')).toBe('en');
  });
});
