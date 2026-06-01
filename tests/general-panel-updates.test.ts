import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';

const { setGeneral } = vi.hoisted(() => ({
  setGeneral: vi.fn(
    async (_general: { check_updates_on_startup: boolean }) =>
      ({ status: 'ok', data: null }) as const,
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
  },
}));
vi.mock('$lib/theme/state.svelte', () => ({
  themeState: { pref: 'system' },
  setThemePref: vi.fn(),
}));
vi.mock('$lib/onboarding/state.svelte', () => ({ replayTour: vi.fn() }));

import GeneralPanel from '$lib/settings/GeneralPanel.svelte';

describe('GeneralPanel updates toggle', () => {
  it('persists check_updates_on_startup when toggled', async () => {
    const { findByTestId } = render(GeneralPanel);
    const cb = (await findByTestId('updates-toggle')) as HTMLInputElement;
    expect(cb.checked).toBe(true);
    await fireEvent.click(cb);
    expect(setGeneral).toHaveBeenCalled();
    expect(setGeneral.mock.calls.at(-1)?.[0].check_updates_on_startup).toBe(false);
  });
});
