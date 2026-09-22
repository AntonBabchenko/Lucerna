import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, type Mock, vi } from 'vitest';

// Settings → Updates once an update is offered: the page says what the button does, refuses
// inline (with Check again) while something runs, and follows the real stage while installing.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        general: {
          hide_to_tray_during_game: false,
          theme: 'system',
          check_updates_on_startup: true,
          gpu_preference: 'auto',
        },
      },
    }),
    appSettingsSetGeneral: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    updateCheck: vi.fn(),
    restartBlocked: vi.fn(),
    updateInstall: vi.fn().mockReturnValue(new Promise(() => {})),
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
import { updateInstalling, updatePhase, updateState } from '$lib/update/state.svelte';

const flush = () => new Promise((r) => setTimeout(r, 0));
const AVAILABLE = {
  status: 'ok',
  data: {
    available: true,
    current: '0.24.0',
    latest: '0.25.0',
    release_url: 'https://github.com/x/releases/tag/v0.25.0',
    installer: { url: 'https://cdn.example/Lucerna_x64-setup.exe', name: 'x', size: 1 },
    sha256sums: null,
    cosign_bundle: null,
  },
};

async function offerAnUpdate() {
  (commands.updateCheck as Mock).mockResolvedValue(AVAILABLE);
  render(UpdatesPanel);
  await flush();
  await fireEvent.click(screen.getByTestId('check-updates-btn'));
  await flush();
  await flush();
}
const updateBtn = () => screen.getByTestId('update-now-btn') as HTMLButtonElement;

beforeEach(() => {
  vi.clearAllMocks();
  updateState.value = null;
  updateInstalling.value = false;
  updatePhase.value = null;
  (commands.restartBlocked as Mock).mockResolvedValue('none');
});

describe('UpdatesPanel — the update action', () => {
  it('says what the button does, next to it', async () => {
    await offerAnUpdate();
    expect(screen.getByText(/closes and comes back on the new version/)).toBeTruthy();
    expect(updateBtn().disabled).toBe(false);
  });

  it('asks whether anything runs once an update is offered, and refuses inline with Check again', async () => {
    (commands.restartBlocked as Mock).mockResolvedValue('running');
    await offerAnUpdate();
    expect(commands.restartBlocked).toHaveBeenCalledTimes(1);
    expect(updateBtn().disabled).toBe(true);
    // The sentence is the panel's own: it speaks of closing to update, not of moving the folder.
    expect(screen.getByText(/Close Minecraft and stop your servers first/)).toBeTruthy();
    expect(screen.queryByText(/moving the data folder/)).toBeNull();

    (commands.restartBlocked as Mock).mockResolvedValue('none');
    await fireEvent.click(screen.getByRole('button', { name: 'Check again' }));
    await flush();
    expect(updateBtn().disabled).toBe(false);
    expect(screen.queryByRole('button', { name: 'Check again' })).toBeNull();
  });

  it('stays blocked, and says so, while it could not tell', async () => {
    (commands.restartBlocked as Mock).mockRejectedValue(new Error('ipc down'));
    await offerAnUpdate();
    expect(updateBtn().disabled).toBe(true);
    expect(screen.getByText(/couldn't check whether a game or a server is running/)).toBeTruthy();
  });

  it('follows the real stage while installing', async () => {
    await offerAnUpdate();
    updateInstalling.value = true;
    await flush();
    expect(updateBtn().textContent).toMatch(/Updating…/);
    updatePhase.value = 'downloading';
    await flush();
    expect(updateBtn().textContent).toMatch(/Downloading…/);
    updatePhase.value = 'verifying';
    await flush();
    expect(updateBtn().textContent).toMatch(/Verifying…/);
    updatePhase.value = 'launching';
    await flush();
    expect(updateBtn().textContent).toMatch(/Starting the installer…/);
    expect(updateBtn().textContent).not.toMatch(/Installing…/);
  });
});
