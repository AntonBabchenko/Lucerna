// tests/game-panel.test.ts
import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, test, vi } from 'vitest';

const appSettingsGet = vi.fn().mockResolvedValue({
  status: 'ok',
  data: {
    general: {
      hide_to_tray_during_game: false,
      theme: 'system',
      check_updates_on_startup: true,
      gpu_preference: 'auto',
    },
  },
});
const appSettingsSetGeneral = vi.fn().mockResolvedValue({ status: 'ok', data: null });

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: (...a: unknown[]) => appSettingsGet(...a),
    appSettingsSetGeneral: (...a: unknown[]) => appSettingsSetGeneral(...a),
    gpuCapability: vi.fn().mockResolvedValue({ status: 'ok', data: { kind: 'unsupported' } }),
  },
}));

import GamePanel from '../src/lib/settings/GamePanel.svelte';

beforeEach(() => appSettingsSetGeneral.mockClear());

describe('GamePanel', () => {
  test('renders the tray toggle', () => {
    const { container } = render(GamePanel);
    const cb = container.querySelector('[data-testid="tray-toggle"]');
    expect(cb).not.toBeNull();
    expect(cb?.getAttribute('type')).toBe('checkbox');
  });

  test('toggling tray persists only tray + gpu fields (fresh RMW)', async () => {
    render(GamePanel);
    await vi.waitFor(() => expect(appSettingsGet).toHaveBeenCalled());
    await fireEvent.click(screen.getByTestId('tray-toggle'));
    await vi.waitFor(() => expect(appSettingsSetGeneral).toHaveBeenCalled());
    const arg = appSettingsSetGeneral.mock.calls.at(-1)?.[0];
    expect(arg.hide_to_tray_during_game).toBe(true);
    // Sibling fields from the fresh read are preserved, not dropped.
    expect(arg.check_updates_on_startup).toBe(true);
  });
});
