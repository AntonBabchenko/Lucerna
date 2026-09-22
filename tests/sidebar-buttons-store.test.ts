import { beforeEach, describe, expect, it, vi } from 'vitest';

// Sidebar-button visibility persists through the one settings contract: a
// field-level patch of the hidden list, a rollback when refused, and never a
// rollback of a newer toggle.
const { appSettingsGet, appSettingsSetGeneral, appSettingsPatchGeneral, sampleGeneral } =
  vi.hoisted(() => {
    const sampleGeneral = {
      hide_to_tray_during_game: false,
      theme: 'system',
      check_updates_on_startup: true,
      language: 'system',
      explanation_level: 'basic',
      compact_mode: false,
      hidden_sidebar_buttons: [] as string[],
    };
    return {
      sampleGeneral,
      appSettingsSetGeneral: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
      appSettingsGet: vi.fn().mockResolvedValue({ status: 'ok', data: { general: sampleGeneral } }),
      appSettingsPatchGeneral: vi.fn(),
    };
  });

vi.mock('$lib/ipc/bindings', () => ({
  commands: { appSettingsGet, appSettingsSetGeneral, appSettingsPatchGeneral },
}));

import { initSidebarButtons, isVisible, setHidden } from '$lib/layout/sidebar-buttons.svelte';
import { __resetAppSettingsForTest, loadAppSettings } from '$lib/settings/app-settings.svelte';

const REFUSED = { status: 'error', error: { kind: 'io', path: 'app.json', details: 'x' } };

describe('sidebar button visibility store', () => {
  beforeEach(async () => {
    initSidebarButtons([]);
    appSettingsGet.mockClear();
    appSettingsSetGeneral.mockClear();
    appSettingsPatchGeneral.mockReset().mockImplementation(async (p: object) => ({
      status: 'ok',
      data: { ...sampleGeneral, ...p },
    }));
    __resetAppSettingsForTest();
    await loadAppSettings();
  });

  it('initSidebarButtons seeds the hidden set without persisting', () => {
    initSidebarButtons(['gallery']);
    expect(isVisible('gallery')).toBe(false);
    expect(appSettingsPatchGeneral).not.toHaveBeenCalled();
  });

  it('setHidden(true) hides the button and patches only hidden_sidebar_buttons', async () => {
    await setHidden('gallery', true);
    expect(isVisible('gallery')).toBe(false);
    expect(appSettingsPatchGeneral).toHaveBeenCalledTimes(1);
    expect(appSettingsPatchGeneral).toHaveBeenCalledWith({ hidden_sidebar_buttons: ['gallery'] });
  });

  it('setHidden(false) re-shows the button and patches the shrunken list', async () => {
    initSidebarButtons(['gallery', 'logs']);
    await setHidden('gallery', false);
    expect(isVisible('gallery')).toBe(true);
    expect(appSettingsPatchGeneral).toHaveBeenCalledWith({ hidden_sidebar_buttons: ['logs'] });
  });

  it('does not duplicate an already-hidden id', async () => {
    initSidebarButtons(['gallery']);
    await setHidden('gallery', true);
    expect(appSettingsPatchGeneral.mock.calls[0][0].hidden_sidebar_buttons).toEqual(['gallery']);
  });

  it('rolls back and keeps the button visible when the patch is refused', async () => {
    appSettingsPatchGeneral.mockResolvedValueOnce(REFUSED);
    await setHidden('logs', true);
    expect(isVisible('logs')).toBe(true); // rolled back
  });

  it('a stale failure does not revert a newer toggle', async () => {
    // Hide A (refused, slow), then hide B (fast): A's rollback must not
    // un-hide B, which is what a "revert to the set before A" did.
    let failFirst!: (v: unknown) => void;
    appSettingsPatchGeneral.mockReturnValueOnce(new Promise((r) => (failFirst = r)));
    const first = setHidden('gallery', true);
    const second = setHidden('logs', true);
    await second;
    failFirst(REFUSED);
    await first;
    expect(isVisible('logs')).toBe(false);
  });
});
