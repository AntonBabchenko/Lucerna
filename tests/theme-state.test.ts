import { beforeEach, describe, expect, it, vi } from 'vitest';

// The theme setter persists through the one settings contract: a field-level
// patch, a rollback of every surface it painted when the patch is refused,
// and never a rollback of a newer pick.
const appSettingsGet = vi.fn();
const appSettingsSetGeneral = vi.fn();
const appSettingsPatchGeneral = vi.fn();
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: () => appSettingsGet(),
    appSettingsSetGeneral: (g: unknown) => appSettingsSetGeneral(g),
    appSettingsPatchGeneral: (p: unknown) => appSettingsPatchGeneral(p),
  },
}));

import { __resetAppSettingsForTest, loadAppSettings } from '$lib/settings/app-settings.svelte';
import { setThemePref, themeState } from '$lib/theme/state.svelte';

const GENERAL = { theme: 'system', language: 'system' };
const REFUSED = { status: 'error', error: { kind: 'io', path: 'app.json', details: 'x' } };

beforeEach(async () => {
  appSettingsGet.mockReset().mockResolvedValue({ status: 'ok', data: { general: GENERAL } });
  appSettingsSetGeneral.mockReset().mockResolvedValue({ status: 'ok', data: null });
  appSettingsPatchGeneral
    .mockReset()
    .mockImplementation(async (p: object) => ({ status: 'ok', data: { ...GENERAL, ...p } }));
  __resetAppSettingsForTest();
  await loadAppSettings();
  themeState.pref = 'system';
  localStorage.clear();
  document.documentElement.classList.remove('dark');
});

describe('setThemePref', () => {
  it('applies the pick to the rune, the mirror and the html class, then patches only the theme', async () => {
    await setThemePref('dark');
    expect(themeState.pref).toBe('dark');
    expect(localStorage.getItem('theme')).toBe('dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);
    expect(appSettingsPatchGeneral).toHaveBeenCalledTimes(1);
    expect(appSettingsPatchGeneral).toHaveBeenCalledWith({ theme: 'dark' });
  });

  it('rolls all three surfaces back when the patch is refused', async () => {
    appSettingsPatchGeneral.mockResolvedValueOnce(REFUSED);
    await setThemePref('dark');
    expect(themeState.pref).toBe('system');
    // app.html's anti-FOUC script reads this before Svelte boots: a stale 'dark'
    // would paint the next launch in a theme app.json does not name.
    expect(localStorage.getItem('theme')).toBe('system');
    expect(document.documentElement.classList.contains('dark')).toBe(false);
  });

  it('a stale failure does not clobber a newer pick', async () => {
    // Pick 1 (dark) is refused; pick 2 (light) succeeds and is on screen by
    // then. Rolling pick 1 back to 'system' would silently undo 'light'.
    appSettingsPatchGeneral.mockResolvedValueOnce(REFUSED);
    const first = setThemePref('dark');
    const second = setThemePref('light');
    await Promise.all([first, second]);
    expect(themeState.pref).toBe('light');
  });
});
