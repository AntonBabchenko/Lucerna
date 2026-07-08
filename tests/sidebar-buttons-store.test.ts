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
    await setHidden('servers', true);
    expect(isVisible('servers')).toBe(false);
    expect(appSettingsSetGeneral).toHaveBeenCalledTimes(1);
    expect(appSettingsSetGeneral.mock.calls[0][0]).toMatchObject({
      hidden_sidebar_buttons: ['servers'],
    });
  });

  it('setHidden(false) re-shows the button and persists the shrunken list', async () => {
    initSidebarButtons(['servers', 'logs']);
    await setHidden('servers', false);
    expect(isVisible('servers')).toBe(true);
    expect(appSettingsSetGeneral.mock.calls[0][0]).toMatchObject({
      hidden_sidebar_buttons: ['logs'],
    });
  });

  it('does not duplicate an already-hidden id', async () => {
    initSidebarButtons(['servers']);
    await setHidden('servers', true);
    expect(appSettingsSetGeneral.mock.calls[0][0].hidden_sidebar_buttons).toEqual(['servers']);
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
