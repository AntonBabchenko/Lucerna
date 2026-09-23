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
const appSettingsPatchGeneral = vi.fn().mockImplementation(async (p: object) => ({
  status: 'ok',
  data: { gpu_preference: 'auto', ...p },
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: (...a: unknown[]) => appSettingsGet(...a),
    appSettingsSetGeneral: (...a: unknown[]) => appSettingsSetGeneral(...a),
    appSettingsPatchGeneral: (...a: unknown[]) => appSettingsPatchGeneral(...a),
    gpuCapability: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { mechanism: 'none', capability: { kind: 'unsupported' } },
    }),
  },
}));

import {
  __resetAppSettingsForTest,
  loadAppSettings,
} from '../src/lib/settings/app-settings.svelte';
import GamePanel from '../src/lib/settings/GamePanel.svelte';
import { describedText } from './test-utils/aria';

async function mount() {
  __resetAppSettingsForTest();
  await loadAppSettings();
  return render(GamePanel);
}

beforeEach(() => {
  appSettingsSetGeneral.mockClear();
  appSettingsPatchGeneral.mockClear();
});

describe('GamePanel', () => {
  test('renders the tray toggle', async () => {
    const { container } = await mount();
    const cb = container.querySelector('[data-testid="tray-toggle"]');
    expect(cb).not.toBeNull();
    expect(cb?.getAttribute('type')).toBe('checkbox');
  });

  test('toggling tray patches only the toggled field', async () => {
    await mount();
    await fireEvent.click(screen.getByTestId('tray-toggle'));
    await vi.waitFor(() => expect(appSettingsPatchGeneral).toHaveBeenCalled());
    // One field, nothing else: a sibling panel's field can never be clobbered.
    expect(appSettingsPatchGeneral).toHaveBeenCalledWith({ hide_to_tray_during_game: true });
  });

  test('the tray checkbox is named by its title and described by its sentence', async () => {
    await mount();
    // Today the <label> wraps title AND sentence, so the name is the whole paragraph.
    const cb = screen.getByRole('checkbox', {
      name: 'Hide launcher to tray when Minecraft starts',
    });
    expect(cb.getAttribute('data-testid')).toBe('tray-toggle');
    expect(describedText(cb)).toContain('system tray');
  });
});
