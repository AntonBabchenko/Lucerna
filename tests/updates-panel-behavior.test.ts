import { fireEvent, render } from '@testing-library/svelte';
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
