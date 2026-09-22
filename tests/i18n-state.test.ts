import { get } from 'svelte/store';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// The locale setter persists through the one settings contract: a field-level
// patch, and a rollback of the rune, the live locale and the mirror when the
// patch is refused.
const appSettingsGet = vi.fn();
const appSettingsSetGeneral = vi.fn().mockResolvedValue({ status: 'ok', data: null });
const appSettingsPatchGeneral = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: (...a: unknown[]) => appSettingsGet(...a),
    appSettingsSetGeneral: (...a: unknown[]) => appSettingsSetGeneral(...a),
    appSettingsPatchGeneral: (...a: unknown[]) => appSettingsPatchGeneral(...a),
  },
}));

import { locale } from '../src/lib/i18n';
import { initLocale, langPref, setLocalePref } from '../src/lib/i18n/state.svelte';
import {
  __resetAppSettingsForTest,
  loadAppSettings,
} from '../src/lib/settings/app-settings.svelte';

const GENERAL = { hide_to_tray_during_game: false, theme: 'system', language: 'system' };

beforeEach(async () => {
  appSettingsGet.mockReset().mockResolvedValue({ status: 'ok', data: { general: GENERAL } });
  appSettingsSetGeneral.mockClear();
  appSettingsPatchGeneral
    .mockReset()
    .mockImplementation(async (p: object) => ({ status: 'ok', data: { ...GENERAL, ...p } }));
  __resetAppSettingsForTest();
  await loadAppSettings();
  localStorage.clear();
});

describe('i18n state', () => {
  it('initLocale sets pref, resolved locale, and mirrors localStorage', () => {
    initLocale('ru');
    expect(langPref.value).toBe('ru');
    expect(get(locale)).toBe('ru');
    expect(localStorage.getItem('locale')).toBe('ru');
  });

  it('setLocalePref switches the live locale and patches only the language', async () => {
    initLocale('en');
    await setLocalePref('ru');
    expect(langPref.value).toBe('ru');
    expect(get(locale)).toBe('ru');
    expect(localStorage.getItem('locale')).toBe('ru');
    expect(appSettingsPatchGeneral).toHaveBeenCalledTimes(1);
    expect(appSettingsPatchGeneral).toHaveBeenCalledWith({ language: 'ru' });
  });

  it('rolls the locale, the rune and the localStorage mirror back when the patch is refused', async () => {
    initLocale('en');
    appSettingsPatchGeneral.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: 'app.json', details: 'x' },
    });

    await setLocalePref('ru');

    expect(langPref.value).toBe('en');
    expect(get(locale)).toBe('en');
    // The mirror is what the next launch's anti-FOUC path reads; a stale 'ru'
    // there opens the UI in a language app.json does not name.
    expect(localStorage.getItem('locale')).toBe('en');
  });
});
