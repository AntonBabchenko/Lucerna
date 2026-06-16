import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

const { setGeneral, updateCheck } = vi.hoisted(() => ({
  setGeneral: vi.fn(
    async (_general: { check_updates_on_startup: boolean }) =>
      ({ status: 'ok', data: null }) as const,
  ),
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
    updateCheck,
    gpuCapability: vi.fn().mockResolvedValue({ status: 'ok', data: { kind: 'unsupported' } }),
  },
}));

import UpdatesPanel from '$lib/settings/UpdatesPanel.svelte';

describe('UpdatesPanel updates toggle', () => {
  it('persists check_updates_on_startup when toggled', async () => {
    const { findByTestId } = render(UpdatesPanel);
    const cb = (await findByTestId('updates-toggle')) as HTMLInputElement;
    expect(cb.checked).toBe(true);
    await fireEvent.click(cb);
    // RMW: the panel re-reads settings before writing, so the persist resolves
    // asynchronously after the click. Assert that it was called and the last
    // call disabled the flag, rather than relying on synchronous ordering.
    await vi.waitFor(() => expect(setGeneral).toHaveBeenCalled());
    expect(setGeneral.mock.calls.at(-1)?.[0].check_updates_on_startup).toBe(false);
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
