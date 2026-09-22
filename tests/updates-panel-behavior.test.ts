import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const { setGeneral, patchGeneral, updateCheck } = vi.hoisted(() => ({
  setGeneral: vi.fn(
    async (_general: { check_updates_on_startup: boolean }) =>
      ({ status: 'ok', data: null }) as const,
  ),
  patchGeneral: vi.fn(async (p: { check_updates_on_startup?: boolean }) => ({
    status: 'ok' as const,
    data: {
      hide_to_tray_during_game: false,
      theme: 'system',
      check_updates_on_startup: true,
      ...p,
    },
  })),
  updateCheck: vi.fn(
    async () =>
      ({ status: 'ok', data: { available: false, current: '0.9.0', latest: '0.9.0' } }) as const,
  ),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: vi.fn(async () => ({
      status: 'ok',
      data: {
        general: {
          hide_to_tray_during_game: false,
          theme: 'system',
          check_updates_on_startup: true,
        },
      },
    })),
    appSettingsSetGeneral: setGeneral,
    appSettingsPatchGeneral: patchGeneral,
    updateCheck,
    // The update gate asks once an update is offered.
    restartBlocked: vi.fn().mockResolvedValue('none'),
    gpuCapability: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { mechanism: 'none', capability: { kind: 'unsupported' } },
    }),
  },
}));

import { __resetAppSettingsForTest, loadAppSettings } from '$lib/settings/app-settings.svelte';
import UpdatesPanel from '$lib/settings/UpdatesPanel.svelte';

beforeEach(async () => {
  __resetAppSettingsForTest();
  await loadAppSettings();
});

describe('UpdatesPanel updates toggle', () => {
  it('persists check_updates_on_startup when toggled', async () => {
    const { findByTestId } = render(UpdatesPanel);
    const cb = (await findByTestId('updates-toggle')) as HTMLInputElement;
    // The box shows no value until the setting has been read (it is disabled meanwhile).
    await vi.waitFor(() => expect(cb.disabled).toBe(false));
    expect(cb.checked).toBe(true);
    await fireEvent.click(cb);
    // The patch is one field and resolves asynchronously after the click.
    await vi.waitFor(() => expect(patchGeneral).toHaveBeenCalled());
    expect(patchGeneral).toHaveBeenCalledWith({ check_updates_on_startup: false });
  });

  it('manual check reports up-to-date inline', async () => {
    const { findByTestId } = render(UpdatesPanel);
    const btn = await findByTestId('check-updates-btn');
    await fireEvent.click(btn);
    expect(updateCheck).toHaveBeenCalled();
    const status = await findByTestId('update-status');
    expect(status.textContent).toContain('latest version');
  });
});

const AVAILABLE = {
  status: 'ok' as const,
  data: {
    available: true,
    current: '0.9.0',
    latest: '0.10.0',
    release_url: 'https://github.com/x/releases/tag/v0.10.0',
    installer: { url: 'https://cdn.example/Lucerna_x64-setup.exe', name: 'x', size: 1 },
    sha256sums: null,
    cosign_bundle: null,
  },
};

describe('UpdatesPanel — the check result is a live region', () => {
  it('hides the refresh icon while a check is running', async () => {
    updateCheck.mockReturnValueOnce(new Promise(() => {}));
    const { findByTestId } = render(UpdatesPanel);
    const btn = await findByTestId('check-updates-btn');
    expect(btn.querySelector('svg')).not.toBeNull();
    await fireEvent.click(btn);
    expect(btn.querySelector('svg')).toBeNull();
  });

  it('up to date → status, a failed check → alert, an update → status, all inside update-status', async () => {
    const { findByTestId } = render(UpdatesPanel);
    const btn = await findByTestId('check-updates-btn');

    await fireEvent.click(btn);
    await vi.waitFor(() => {
      const box = screen.getByTestId('update-status');
      expect(box.querySelector('[role="status"]')?.textContent).toContain('latest version');
      expect(box.querySelector('[role="alert"]')).toBeNull();
    });

    updateCheck.mockRejectedValueOnce(new Error('ipc channel closed'));
    await fireEvent.click(btn);
    await vi.waitFor(() => {
      const box = screen.getByTestId('update-status');
      expect(box.querySelector('[role="alert"]')?.textContent).toContain('ipc channel closed');
    });

    updateCheck.mockResolvedValueOnce(AVAILABLE);
    await fireEvent.click(btn);
    await vi.waitFor(() => {
      const box = screen.getByTestId('update-status');
      expect(box.querySelector('[role="status"]')?.textContent).toContain('0.10.0');
      expect(box.querySelector('[role="alert"]')).toBeNull();
    });
  });
});
