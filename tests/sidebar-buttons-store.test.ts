import { beforeEach, describe, expect, it, vi } from 'vitest';

const { appSettingsGet, appSettingsSetGeneral } = vi.hoisted(() => {
  const sampleGeneral = {
    hide_to_tray_during_game: false,
    theme: 'system',
    check_updates_on_startup: true,
    language: 'system',
    explanation_level: 'basic',
    compact_mode: false,
    hidden_sidebar_buttons: [],
  };
  return {
    appSettingsSetGeneral: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    appSettingsGet: vi.fn().mockResolvedValue({ status: 'ok', data: { general: sampleGeneral } }),
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: { appSettingsGet, appSettingsSetGeneral },
}));

import { initSidebarButtons, isVisible, setHidden } from '$lib/layout/sidebar-buttons.svelte';

describe('sidebar button visibility store', () => {
  beforeEach(() => {
    initSidebarButtons([]);
    appSettingsGet.mockClear();
    appSettingsSetGeneral.mockClear();
  });

  it('initSidebarButtons seeds the hidden set without persisting', () => {
    initSidebarButtons(['manage', 'logs']);
    expect(isVisible('manage')).toBe(false);
    expect(isVisible('logs')).toBe(false);
    expect(isVisible('mods')).toBe(true);
    expect(appSettingsSetGeneral).not.toHaveBeenCalled();
  });

  it('setHidden(true) hides the button and persists hidden_sidebar_buttons', async () => {
    await setHidden('gallery', true);
    expect(isVisible('gallery')).toBe(false);
    expect(appSettingsSetGeneral).toHaveBeenCalledTimes(1);
    expect(appSettingsSetGeneral.mock.calls[0][0]).toMatchObject({
      hidden_sidebar_buttons: ['gallery'],
    });
  });

  it('setHidden(false) re-shows the button and persists the shrunken list', async () => {
    initSidebarButtons(['gallery', 'logs']);
    await setHidden('gallery', false);
    expect(isVisible('gallery')).toBe(true);
    expect(appSettingsSetGeneral.mock.calls[0][0]).toMatchObject({
      hidden_sidebar_buttons: ['logs'],
    });
  });

  it('does not duplicate an already-hidden id', async () => {
    initSidebarButtons(['gallery']);
    await setHidden('gallery', true);
    expect(appSettingsSetGeneral.mock.calls[0][0].hidden_sidebar_buttons).toEqual(['gallery']);
  });

  it('rolls back and keeps the button visible when persistence fails', async () => {
    appSettingsSetGeneral.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', details: 'x' },
    });
    await setHidden('logs', true);
    expect(isVisible('logs')).toBe(true); // rolled back
  });

  it('rolls back and never writes when the settings read fails', async () => {
    appSettingsGet.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', details: 'x' },
    });
    await setHidden('logs', true);
    expect(isVisible('logs')).toBe(true); // rolled back before the write
    expect(appSettingsSetGeneral).not.toHaveBeenCalled();
  });
});
