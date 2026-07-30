// Settings → Game gains the "show status for my saved servers" permission.
// It is the consent gate for the whole ping feature, so two things are pinned
// here: it starts OFF, and the IP-exposure consequence is stated next to it
// (not buried in a doc). The panel owns only its own GeneralSettings fields, so
// the save assertion also proves it round-trips through a fresh read.

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
// Static import on purpose: a dynamic `await import()` of a .svelte module
// inside a test body never resolves under this vitest setup. `vi.mock` is
// hoisted above imports anyway, so the panel still sees the mocked bindings.
import GamePanel from '$lib/settings/GamePanel.svelte';

const appSettingsGet = vi.fn();
const appSettingsSetGeneral = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: () => appSettingsGet(),
    appSettingsSetGeneral: (g: unknown) => appSettingsSetGeneral(g),
    gpuCapability: () => Promise.resolve({ status: 'ok', data: { kind: 'unsupported' } }),
  },
}));

function general(over: Record<string, unknown> = {}) {
  return {
    hide_to_tray_during_game: false,
    theme: 'system',
    check_updates_on_startup: true,
    gpu_preference: 'auto',
    allow_server_ping: false,
    ...over,
  };
}

describe('Settings → Game: saved-server status permission', () => {
  beforeEach(() => {
    locale.set('en');
    appSettingsGet.mockResolvedValue({ status: 'ok', data: { general: general() } });
    appSettingsSetGeneral.mockResolvedValue({ status: 'ok', data: null });
  });

  it('starts off and persists when switched on', async () => {
    render(GamePanel);
    const toggle = await waitFor(
      () => screen.getByTestId('server-ping-toggle') as HTMLInputElement,
    );
    expect(toggle.checked).toBe(false);
    await fireEvent.click(toggle);
    await waitFor(() =>
      expect(appSettingsSetGeneral).toHaveBeenCalledWith(
        expect.objectContaining({ allow_server_ping: true }),
      ),
    );
  });

  it('reflects an already-enabled permission', async () => {
    appSettingsGet.mockResolvedValue({
      status: 'ok',
      data: { general: general({ allow_server_ping: true }) },
    });
    render(GamePanel);
    await waitFor(() =>
      expect((screen.getByTestId('server-ping-toggle') as HTMLInputElement).checked).toBe(true),
    );
  });

  it('states the IP-exposure consequence next to the toggle', async () => {
    render(GamePanel);
    await waitFor(() => screen.getByTestId('server-ping-toggle'));
    expect(screen.getByText(/IP address/i)).toBeTruthy();
  });

  it('does not clobber a sibling panel field when saving', async () => {
    // The read-modify-write must preserve fields this panel does not own.
    appSettingsGet.mockResolvedValue({
      status: 'ok',
      data: { general: general({ language: 'ru', compact_mode: true }) },
    });
    render(GamePanel);
    const toggle = await waitFor(
      () => screen.getByTestId('server-ping-toggle') as HTMLInputElement,
    );
    await fireEvent.click(toggle);
    await waitFor(() =>
      expect(appSettingsSetGeneral).toHaveBeenCalledWith(
        expect.objectContaining({ language: 'ru', compact_mode: true, allow_server_ping: true }),
      ),
    );
  });
});
