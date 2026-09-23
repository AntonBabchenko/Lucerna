// tests/game-panel.test.ts
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, test, vi } from 'vitest';

const appSettingsGet = vi.fn().mockResolvedValue({
  status: 'ok',
  data: {
    general: {
      hide_to_tray_during_game: false,
      game_start_window: 'minimise',
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
const appBuildInfo = vi.fn().mockResolvedValue({
  version: '0.25.0',
  build: { kind: 'local' },
  commit: null,
  fork: null,
  os: 'windows',
  arch: 'x86_64',
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: (...a: unknown[]) => appSettingsGet(...a),
    appSettingsSetGeneral: (...a: unknown[]) => appSettingsSetGeneral(...a),
    appSettingsPatchGeneral: (...a: unknown[]) => appSettingsPatchGeneral(...a),
    appBuildInfo: () => appBuildInfo(),
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
  appBuildInfo.mockClear();
});

describe('GamePanel', () => {
  const group = () => screen.getByRole('group', { name: 'When a game starts' });

  test('offers what the window does when a game starts, with the current choice pressed', async () => {
    await mount();
    const g = group();
    expect(within(g).getByRole('button', { name: 'Minimise' }).getAttribute('aria-pressed')).toBe(
      'true',
    );
    expect(within(g).getByRole('button', { name: 'Keep open' }).getAttribute('aria-pressed')).toBe(
      'false',
    );
    expect(within(g).getByRole('button', { name: 'Hide to tray' })).toBeTruthy();
  });

  test('picking a choice patches only the window field', async () => {
    await mount();
    await fireEvent.click(within(group()).getByRole('button', { name: 'Hide to tray' }));
    await vi.waitFor(() => expect(appSettingsPatchGeneral).toHaveBeenCalled());
    // One field, nothing else: a sibling panel's field can never be clobbered.
    expect(appSettingsPatchGeneral).toHaveBeenCalledWith({ game_start_window: 'hide_to_tray' });
  });

  test('the tray caveat is readable before Hide to tray is chosen, and says which choice it is about', async () => {
    await mount();
    // The consequence is read BEFORE committing: activation follows focus. It must
    // name its option, or it reads as a description of the one that is pressed.
    const described = describedText(group());
    expect(described).toContain('system tray');
    expect(described).toContain('Hide to tray:');
  });

  test('the Linux minimise note shows only on Linux', async () => {
    await mount();
    await vi.waitFor(() => expect(appBuildInfo).toHaveBeenCalledTimes(1));
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByText(/On some Linux desktops/)).toBeNull();
  });

  test('on Linux, it says the launcher may not come back by itself', async () => {
    appBuildInfo.mockResolvedValueOnce({
      version: '0.25.0',
      build: { kind: 'local' },
      commit: null,
      fork: null,
      os: 'linux',
      arch: 'x86_64',
    });
    await mount();
    expect(await screen.findByText(/On some Linux desktops/)).toBeTruthy();
  });
});
