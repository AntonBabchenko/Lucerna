import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, type Mock, vi } from 'vitest';

// A failed theme save no longer snaps back in silence: the message sits under
// the theme picker and clears on the next successful pick.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: vi.fn(),
    appSettingsSetGeneral: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    appSettingsPatchGeneral: vi.fn(),
  },
}));

import { commands } from '$lib/ipc/bindings';
import AppearancePanel from '$lib/settings/AppearancePanel.svelte';
import { __resetAppSettingsForTest, loadAppSettings } from '$lib/settings/app-settings.svelte';
import { themeState } from '$lib/theme/state.svelte';

const GENERAL = { theme: 'system', language: 'system', hidden_sidebar_buttons: [] };
const get = commands.appSettingsGet as Mock;
const patch = commands.appSettingsPatchGeneral as Mock;
const flush = () => new Promise((r) => setTimeout(r, 0));

beforeEach(async () => {
  vi.clearAllMocks();
  __resetAppSettingsForTest();
  get.mockResolvedValue({ status: 'ok', data: { general: GENERAL } });
  patch.mockImplementation(async (p: object) => ({ status: 'ok', data: { ...GENERAL, ...p } }));
  await loadAppSettings();
  themeState.pref = 'system';
  localStorage.clear();
});

describe('AppearancePanel — a failed save says so', () => {
  it('a refused theme save snaps back AND says so under the picker; the next success clears it', async () => {
    patch.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: 'app.json', details: 'locked' },
    });
    render(AppearancePanel);
    await fireEvent.click(screen.getByTestId('theme-dark'));
    await flush();
    await flush();
    expect((screen.getByTestId('theme-system') as HTMLInputElement).checked).toBe(true);
    const line = screen.getByTestId('save-failure-theme');
    expect(line.textContent).toContain("wasn't saved");
    expect(line.textContent).toContain('locked');
    await fireEvent.click(screen.getByTestId('theme-light'));
    await waitFor(() =>
      expect(screen.getByTestId('save-failure-theme').textContent ?? '').toBe(''),
    );
  });
});
