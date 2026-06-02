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
import { langPref, initLocale, setLocalePref } from '../src/lib/i18n/state.svelte';

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
});
