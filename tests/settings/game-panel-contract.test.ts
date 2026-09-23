import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, type Mock, vi } from 'vitest';

// Settings → Game on the one settings contract: no value and no live control
// until the settings are read; "couldn't read" + Retry after a failed read;
// a flip patches only its own field; a failed save is said next to the control
// that failed — and a consent revoke that was REFUSED says the channel is
// still on, while one the transport lost does not claim to know.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: vi.fn(),
    appSettingsSetGeneral: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    appSettingsPatchGeneral: vi.fn(),
    gpuCapability: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { mechanism: 'none', capability: { kind: 'unsupported' } },
    }),
  },
}));

import { commands } from '$lib/ipc/bindings';
import { __resetAppSettingsForTest, loadAppSettings } from '$lib/settings/app-settings.svelte';
import GamePanel from '$lib/settings/GamePanel.svelte';

const GENERAL = {
  hide_to_tray_during_game: false,
  game_start_window: 'keep',
  theme: 'system',
  check_updates_on_startup: true,
  gpu_preference: 'auto',
  allow_server_ping: true,
  language: 'ru',
  compact_mode: true,
};
const get = commands.appSettingsGet as Mock;
const patch = commands.appSettingsPatchGeneral as Mock;
const flush = () => new Promise((r) => setTimeout(r, 0));
const REFUSED = { status: 'error', error: { kind: 'io', path: 'app.json', details: 'locked' } };
const ping = () => screen.getByTestId('server-ping-toggle') as HTMLInputElement;
const choice = (name: string) =>
  within(screen.getByRole('group', { name: 'When a game starts' })).getByRole('button', {
    name,
  }) as HTMLButtonElement;

beforeEach(() => {
  vi.clearAllMocks();
  __resetAppSettingsForTest();
  get.mockResolvedValue({ status: 'ok', data: { general: GENERAL } });
  patch.mockImplementation(async (p: object) => ({ status: 'ok', data: { ...GENERAL, ...p } }));
});

describe('GamePanel — the settings contract', () => {
  it('shows no value and no live control until the settings are read', async () => {
    let resolve!: (v: unknown) => void;
    get.mockReturnValueOnce(new Promise((r) => (resolve = r)));
    void loadAppSettings();
    render(GamePanel);
    for (const name of ['Keep open', 'Minimise', 'Hide to tray']) {
      expect(choice(name).disabled).toBe(true);
      expect(choice(name).getAttribute('aria-pressed')).toBe('false'); // unknown, not "keep"
    }
    expect(ping().disabled).toBe(true);
    expect(ping().checked).toBe(false); // unknown, not "off"
    resolve({ status: 'ok', data: { general: GENERAL } });
    await waitFor(() => expect(ping().disabled).toBe(false));
    expect(ping().checked).toBe(true);
  });

  it('after a failed read: "couldn\'t read", Retry re-reads, controls stay disabled meanwhile', async () => {
    get.mockResolvedValueOnce(REFUSED);
    await loadAppSettings();
    render(GamePanel);
    expect(screen.getByText(/Couldn't read your settings/)).toBeTruthy();
    expect(ping().disabled).toBe(true);
    await fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(ping().disabled).toBe(false));
    expect(screen.queryByText(/Couldn't read your settings/)).toBeNull();
  });

  it('a flip patches only its own field', async () => {
    await loadAppSettings();
    render(GamePanel);
    await fireEvent.click(choice('Minimise'));
    await flush();
    expect(patch).toHaveBeenCalledWith({ game_start_window: 'minimise' });
  });

  it('a refused start-window save goes back to what is saved and says why, under the picker', async () => {
    await loadAppSettings();
    patch.mockResolvedValue(REFUSED);
    render(GamePanel);
    await fireEvent.click(choice('Hide to tray'));
    await flush();
    await flush();
    expect(choice('Keep open').getAttribute('aria-pressed')).toBe('true');
    expect(choice('Hide to tray').getAttribute('aria-pressed')).toBe('false');
    const line = screen.getByTestId('save-failure-game_start_window');
    expect(line.textContent).toContain('locked');
    // Next to the control that failed: directly after the picker, before its notes.
    const picker = screen.getByTestId('game-start-window');
    expect(picker.compareDocumentPosition(line) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    expect(
      line.compareDocumentPosition(document.getElementById('game-tray-desc') as HTMLElement) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
  });

  it('a refused consent revoke shows the box ON again and says status checks are still on', async () => {
    await loadAppSettings();
    patch.mockResolvedValue(REFUSED);
    render(GamePanel);
    expect(ping().checked).toBe(true);
    await fireEvent.click(ping());
    await flush();
    await flush();
    expect(ping().checked).toBe(true);
    const line = screen.getByTestId('save-failure-allow_server_ping');
    expect(line.textContent).toContain('Status checks are still on');
    expect(line.textContent).toContain('locked');
    // The message sits under the consent, not under the tray control.
    expect(screen.getByTestId('save-failure-game_start_window').textContent?.trim()).toBe('');
  });

  it('an unconfirmed revoke does not claim the channel is still on', async () => {
    await loadAppSettings();
    patch.mockRejectedValue(new Error('ipc channel closed'));
    get.mockResolvedValue({
      status: 'ok',
      data: { general: { ...GENERAL, allow_server_ping: false } },
    });
    render(GamePanel);
    await fireEvent.click(ping());
    await flush();
    await flush();
    const line = screen.getByTestId('save-failure-allow_server_ping');
    expect(line.textContent).toContain("Couldn't confirm");
    expect(line.textContent).not.toContain('still on');
    await waitFor(() => expect(ping().checked).toBe(false)); // the re-read decided
  });
});
