// Mock shape copied from tests/explanation-level.test.ts.
import { beforeEach, describe, expect, it, vi } from 'vitest';

const appSettingsGet = vi.fn();
const appSettingsSetGeneral = vi.fn();
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: () => appSettingsGet(),
    appSettingsSetGeneral: (g: unknown) => appSettingsSetGeneral(g),
  },
}));

import { setThemePref, themeState } from '$lib/theme/state.svelte';

beforeEach(() => {
  appSettingsGet.mockReset().mockResolvedValue({
    status: 'ok',
    data: { general: { theme: 'system', language: 'system' } },
  });
  appSettingsSetGeneral.mockReset().mockResolvedValue({ status: 'ok', data: null });
  themeState.pref = 'system';
  localStorage.clear();
  document.documentElement.classList.remove('dark');
});

describe('setThemePref', () => {
  it('applies the pick to the rune, the mirror and the html class, then persists', async () => {
    await setThemePref('dark');
    expect(themeState.pref).toBe('dark');
    expect(localStorage.getItem('theme')).toBe('dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);
    expect(appSettingsSetGeneral).toHaveBeenCalledTimes(1);
    expect(appSettingsSetGeneral.mock.calls[0][0]).toMatchObject({ theme: 'dark' });
  });

  it('rolls all three surfaces back when the settings read fails', async () => {
    appSettingsGet.mockResolvedValueOnce({ status: 'error', error: { kind: 'io', details: 'x' } });
    await setThemePref('dark');
    expect(themeState.pref).toBe('system');
    // app.html's anti-FOUC script reads this before Svelte boots: a stale 'dark'
    // would paint the next launch in a theme app.json does not name.
    expect(localStorage.getItem('theme')).toBe('system');
    expect(document.documentElement.classList.contains('dark')).toBe(false);
    expect(appSettingsSetGeneral).not.toHaveBeenCalled();
  });

  it('rolls back when the persist write fails', async () => {
    appSettingsSetGeneral.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', details: 'x' },
    });
    await setThemePref('dark');
    expect(themeState.pref).toBe('system');
    expect(localStorage.getItem('theme')).toBe('system');
  });

  it('a stale failure does not clobber a newer pick', async () => {
    // Pick 1 (dark) fails to persist; pick 2 (light) succeeds and lands first on
    // screen. Rolling pick 1 back to 'system' here would silently undo 'light'.
    appSettingsSetGeneral.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', details: 'x' },
    });
    const first = setThemePref('dark');
    const second = setThemePref('light');
    await Promise.all([first, second]);
    expect(themeState.pref).toBe('light');
  });
});
