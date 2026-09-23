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
  // Typed wide enough for both outcomes: the default "up to date" answer and
  // the AVAILABLE one a test swaps in (svelte-check type-checks tests/ too).
  updateCheck: vi.fn(
    async (): Promise<
      | {
          status: 'ok';
          data: {
            available: boolean;
            current: string;
            latest: string;
            release_url?: string;
            installer?: { url: string; name: string; size: number };
            sha256sums?: null;
            cosign_bundle?: null;
          };
        }
      | { status: 'error'; error: { kind: string; details: string } }
    > => ({ status: 'ok', data: { available: false, current: '0.9.0', latest: '0.9.0' } }),
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

  it('states the reason for a failed check once, not twice', async () => {
    // errors.updateCheckFailed already reads "Couldn't check for updates", and
    // settings.general.updates.error wrapped it in "Couldn't check: {message}"
    // — the user read the headline twice before reaching the detail.
    updateCheck.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'update_check_failed', details: 'release has no setup asset' },
    });
    const { findByTestId } = render(UpdatesPanel);
    await fireEvent.click(await findByTestId('check-updates-btn'));
    const status = await findByTestId('update-status');
    const text = status.textContent ?? '';
    expect(text).toContain('release has no setup asset');
    expect(text.match(/Couldn.t check/g) ?? []).toHaveLength(1);
  });

  it('still frames an error that carries no headline of its own', async () => {
    // A network failure formats to a bare sentence; stripping the wrapper for
    // every variant would leave it with no context at all.
    updateCheck.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'network', details: 'connection reset' },
    });
    const { findByTestId } = render(UpdatesPanel);
    await fireEvent.click(await findByTestId('check-updates-btn'));
    const status = await findByTestId('update-status');
    expect(status.textContent ?? '').toMatch(/Couldn.t check/);
  });
});
