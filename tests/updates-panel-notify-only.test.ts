// tests/updates-panel-notify-only.test.ts
// The update action must be honest about the platform: where there is no
// in-app install (Linux/macOS — the backend returns a null installer), the
// button must read "Open release page" and a manual-install hint must show,
// instead of an "Update now" that silently opens a browser and appears to do
// nothing. Where in-app install IS supported (installer present), keep the
// original "Update now" action and no hint.
import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

const { updateCheck } = vi.hoisted(() => ({
  updateCheck: vi.fn(),
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
          gpu_preference: 'auto',
        },
      },
    })),
    appSettingsSetGeneral: vi.fn(async () => ({ status: 'ok', data: null })),
    updateCheck,
  },
}));

import UpdatesPanel from '$lib/settings/UpdatesPanel.svelte';

const asset = { name: 'Lucerna_0.2.0_x64-setup.exe', url: 'https://github.com/x' };

function available(installer: typeof asset | null) {
  return {
    status: 'ok' as const,
    data: {
      current: '0.1.0',
      latest: '0.2.0',
      available: true,
      release_url: 'https://github.com/AntonBabchenko/Lucerna/releases/tag/v0.2.0',
      installer,
      sha256sums: installer,
      cosign_bundle: installer,
    },
  };
}

describe('UpdatesPanel — platform-aware update action', () => {
  it('notify-only (installer=null) offers the release page plus a manual hint', async () => {
    updateCheck.mockResolvedValue(available(null));
    const { findByTestId, queryByTestId } = render(UpdatesPanel);

    await fireEvent.click(await findByTestId('check-updates-btn'));

    const btn = await findByTestId('update-now-btn');
    expect(btn.textContent).toContain('Open release page');
    expect(queryByTestId('update-manual-hint')).not.toBeNull();
  });

  it('in-app install (installer present) keeps "Update now" and shows no manual hint', async () => {
    updateCheck.mockResolvedValue(available(asset));
    const { findByTestId, queryByTestId } = render(UpdatesPanel);

    await fireEvent.click(await findByTestId('check-updates-btn'));

    const btn = await findByTestId('update-now-btn');
    expect(btn.textContent).toContain('Update now');
    expect(queryByTestId('update-manual-hint')).toBeNull();
  });
});
