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
import { __resetAppSettingsForTest, loadAppSettings } from '$lib/settings/app-settings.svelte';
import GamePanel from '$lib/settings/GamePanel.svelte';
import { describedText } from './test-utils/aria';

async function mount() {
  __resetAppSettingsForTest();
  await loadAppSettings();
  return render(GamePanel);
}

const appSettingsGet = vi.fn();
const appSettingsSetGeneral = vi.fn();
const appSettingsPatchGeneral = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: () => appSettingsGet(),
    appSettingsSetGeneral: (g: unknown) => appSettingsSetGeneral(g),
    appSettingsPatchGeneral: (p: unknown) => appSettingsPatchGeneral(p),
    gpuCapability: () =>
      Promise.resolve({
        status: 'ok',
        data: { mechanism: 'none', capability: { kind: 'unsupported' } },
      }),
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
    appSettingsPatchGeneral.mockReset().mockImplementation(async (p: object) => {
      // As the backend does: the block as persisted = the current block ⊕ the patch.
      const cur = await appSettingsGet();
      return { status: 'ok', data: { ...cur.data.general, ...p } };
    });
  });

  it('starts off and persists when switched on', async () => {
    await mount();
    const toggle = await waitFor(
      () => screen.getByTestId('server-ping-toggle') as HTMLInputElement,
    );
    expect(toggle.checked).toBe(false);
    await fireEvent.click(toggle);
    await waitFor(() =>
      expect(appSettingsPatchGeneral).toHaveBeenCalledWith(
        expect.objectContaining({ allow_server_ping: true }),
      ),
    );
  });

  it('reflects an already-enabled permission', async () => {
    appSettingsGet.mockResolvedValue({
      status: 'ok',
      data: { general: general({ allow_server_ping: true }) },
    });
    await mount();
    await waitFor(() =>
      expect((screen.getByTestId('server-ping-toggle') as HTMLInputElement).checked).toBe(true),
    );
  });

  it('states the IP-exposure consequence next to the toggle', async () => {
    await mount();
    await waitFor(() => screen.getByTestId('server-ping-toggle'));
    expect(screen.getByText(/IP address/i)).toBeTruthy();
  });

  it('patches only its own field, so a sibling panel field can never be clobbered', async () => {
    appSettingsGet.mockResolvedValue({
      status: 'ok',
      data: { general: general({ language: 'ru', compact_mode: true }) },
    });
    await mount();
    const toggle = await waitFor(
      () => screen.getByTestId('server-ping-toggle') as HTMLInputElement,
    );
    await fireEvent.click(toggle);
    await waitFor(() =>
      expect(appSettingsPatchGeneral).toHaveBeenCalledWith({ allow_server_ping: true }),
    );
  });

  it('is named by its title and described by the sentence and the privacy line', async () => {
    await mount();
    const cb = await waitFor(() =>
      screen.getByRole('checkbox', { name: 'Show status for my saved servers' }),
    );
    const desc = describedText(cb);
    expect(desc).toContain('player count');
    expect(desc).toContain('IP address');
  });

  it('names the place the list lives in, and keeps the IP consequence', async () => {
    await mount();
    const cb = await waitFor(() =>
      screen.getByRole('checkbox', { name: 'Show status for my saved servers' }),
    );
    const desc = describedText(cb);
    // GAME-04: "this instance's list" named nothing — Settings is global and
    // the list is the Servers dialog.
    expect(desc).toContain('Servers list');
    expect(desc).not.toContain("this instance's");
    expect(desc).toMatch(/IP address/i);
  });

  it('uses the ru word for an instance, not the one reserved for modpacks', async () => {
    locale.set('ru');
    try {
      await mount();
      const cb = await waitFor(() =>
        screen.getByRole('checkbox', { name: 'Показывать статус моих сохранённых серверов' }),
      );
      const desc = describedText(cb);
      expect(desc).toContain('профил');
      expect(desc).not.toContain('этой сборки');
    } finally {
      locale.set('en');
    }
  });
});
