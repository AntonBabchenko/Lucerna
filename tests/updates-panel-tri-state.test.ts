import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, type Mock, vi } from 'vitest';

// The startup-check toggle tells "off" from "could not tell": an unknown value is never shown
// as a confident default, a failed save says so and reverts, and a thrown check recovers.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: vi.fn(),
    appSettingsSetGeneral: vi.fn(),
    updateCheck: vi.fn(),
    restartBlocked: vi.fn().mockResolvedValue('none'),
    updateInstall: vi.fn(),
    updateDismiss: vi.fn(),
  },
  events: {
    downloadProgress: { listen: vi.fn().mockResolvedValue(() => {}) },
    updateInstallPhase: { listen: vi.fn().mockResolvedValue(() => {}) },
  },
}));
vi.mock('$lib/tasks/registry.svelte', () => ({ taskList: () => [], isActiveTask: () => false }));

import { commands } from '$lib/ipc/bindings';
import UpdatesPanel from '$lib/settings/UpdatesPanel.svelte';

const flush = () => new Promise((r) => setTimeout(r, 0));
const general = (check: boolean) => ({
  hide_to_tray_during_game: false,
  theme: 'system',
  check_updates_on_startup: check,
  gpu_preference: 'auto',
});
const toggle = () => screen.getByTestId('updates-toggle') as HTMLInputElement;

beforeEach(() => {
  vi.clearAllMocks();
  (commands.appSettingsGet as Mock).mockResolvedValue({
    status: 'ok',
    data: { general: general(false) },
  });
  (commands.appSettingsSetGeneral as Mock).mockResolvedValue({ status: 'ok', data: null });
});

describe('UpdatesPanel — the startup-check toggle', () => {
  it('shows no value until the setting has been read', async () => {
    let resolve!: (v: unknown) => void;
    (commands.appSettingsGet as Mock).mockReturnValue(new Promise((r) => (resolve = r)));
    render(UpdatesPanel);
    expect(toggle().disabled).toBe(true);
    expect(toggle().checked).toBe(false);
    resolve({ status: 'ok', data: { general: general(true) } });
    await flush();
    expect(toggle().disabled).toBe(false);
    expect(toggle().checked).toBe(true);
  });

  it('when the read fails: disabled, no value, the reason, and Retry re-reads', async () => {
    (commands.appSettingsGet as Mock)
      .mockResolvedValueOnce({
        status: 'error',
        error: { kind: 'io', path: 'app.json', details: 'boom' },
      })
      .mockResolvedValueOnce({ status: 'ok', data: { general: general(true) } });
    render(UpdatesPanel);
    await flush();
    expect(toggle().disabled).toBe(true);
    // Today the hard-coded default rendered as a confident "on" next to the error.
    expect(toggle().checked).toBe(false);
    expect(screen.getByText(/Couldn't read this setting/)).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await flush();
    expect(toggle().disabled).toBe(false);
    expect(toggle().checked).toBe(true);
    expect(screen.queryByText(/Couldn't read this setting/)).toBeNull();
  });

  it('when the save fails: reverts to the confirmed value and says the change was not saved', async () => {
    (commands.appSettingsSetGeneral as Mock).mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: 'app.json', details: 'disk full' },
    });
    render(UpdatesPanel);
    await flush();
    expect(toggle().checked).toBe(false);
    await fireEvent.click(toggle());
    await flush();
    await flush();
    expect(toggle().checked).toBe(false);
    expect(screen.getByText(/wasn't saved/)).toBeTruthy();
  });

  it('a thrown check does not leave the button on "Checking…"', async () => {
    (commands.updateCheck as Mock).mockRejectedValue(new Error('ipc channel closed'));
    render(UpdatesPanel);
    await flush();
    const btn = screen.getByTestId('check-updates-btn') as HTMLButtonElement;
    await fireEvent.click(btn);
    await flush();
    expect(btn.textContent).toMatch(/Check for updates/);
    expect(btn.textContent).not.toMatch(/Checking…/);
    expect(screen.getByTestId('update-status').textContent).toMatch(/ipc channel closed/);
  });
});
