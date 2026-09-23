import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { GpuStatus } from '$lib/ipc/bindings';

// The GPU block tells the user why a control is hidden, never claims "single
// GPU" for "could not tell", keeps a stored choice visible with a way back to
// Automatic, and describes the mechanism of the machine it runs on.
const { gpuCapability, general } = vi.hoisted(() => ({
  gpuCapability: vi.fn(),
  general: {
    hide_to_tray_during_game: false,
    theme: 'system',
    check_updates_on_startup: true,
    gpu_preference: 'auto',
    allow_server_ping: false,
  },
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: vi.fn(async () => ({ status: 'ok', data: { general } })),
    appSettingsSetGeneral: vi.fn(async () => ({ status: 'ok', data: null })),
    appSettingsPatchGeneral: vi.fn(async (p: object) => ({
      status: 'ok',
      data: { ...general, ...p },
    })),
    gpuCapability: () => gpuCapability(),
  },
}));

import { commands } from '$lib/ipc/bindings';
import { __resetAppSettingsForTest, loadAppSettings } from '$lib/settings/app-settings.svelte';
import GamePanel from '$lib/settings/GamePanel.svelte';
import { describedText } from '../test-utils/aria';

async function mount() {
  __resetAppSettingsForTest();
  await loadAppSettings();
  return render(GamePanel);
}

const AVAILABLE: GpuStatus = {
  mechanism: 'windows_registry',
  capability: {
    kind: 'available',
    gpus: [{ name: 'NVIDIA' }, { name: 'Intel' }],
    high: 'NVIDIA',
    low: 'Intel',
  },
};
const status = (
  mechanism: GpuStatus['mechanism'],
  capability: GpuStatus['capability'],
): GpuStatus => ({ mechanism, capability });
const flush = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  vi.clearAllMocks();
  general.gpu_preference = 'auto';
  gpuCapability.mockResolvedValue({ status: 'ok', data: AVAILABLE });
});

describe('GamePanel GPU block', () => {
  it('starts checking — the first frame never says "not available"', async () => {
    let resolve!: (v: unknown) => void;
    gpuCapability.mockReturnValueOnce(new Promise((r) => (resolve = r)));
    await mount();
    // The very first frame claims nothing — synchronously, before any answer. (The spinner
    // itself has an anti-flicker delay, so it is awaited, not asserted on the first frame.)
    expect(screen.queryByTestId('gpu-reason')).toBeNull();
    expect(screen.queryByTestId('gpu-select')).toBeNull();
    await waitFor(() => expect(screen.queryByRole('status')).not.toBeNull());
    resolve({ status: 'ok', data: AVAILABLE });
    await screen.findByTestId('gpu-select');
    await waitFor(() => expect(screen.queryByRole('status')).toBeNull());
  });

  it('shows the dropdown, and the Windows note names the registry key', async () => {
    await mount();
    await screen.findByTestId('gpu-select');
    expect(screen.getByTestId('gpu-note').textContent).toContain('UserGpuPreferences');
  });

  it('the Linux note speaks of PRIME, not of the registry', async () => {
    gpuCapability.mockResolvedValue({
      status: 'ok',
      data: status('linux_env', AVAILABLE.capability),
    });
    await mount();
    await screen.findByTestId('gpu-select');
    expect(screen.getByTestId('gpu-note').textContent).toContain('PRIME');
    expect(screen.getByTestId('gpu-note').textContent).not.toContain('UserGpuPreferences');
  });

  it.each([
    ['single_gpu', 'windows_registry', 'Only one graphics adapter'],
    ['unsupported', 'none', 'macOS picks the GPU itself'],
  ] as const)('%s hides the dropdown and says why', async (kind, mechanism, text) => {
    gpuCapability.mockResolvedValue({ status: 'ok', data: status(mechanism, { kind }) });
    await mount();
    await flush();
    expect(screen.queryByTestId('gpu-select')).toBeNull();
    expect(screen.getByTestId('gpu-reason').textContent).toContain(text);
  });

  it('a failed probe is "could not check" with the details — never "single GPU"', async () => {
    gpuCapability.mockResolvedValue({
      status: 'ok',
      data: status('windows_registry', { kind: 'unknown', details: 'class key: access denied' }),
    });
    await mount();
    await flush();
    const reason = screen.getByTestId('gpu-reason').textContent;
    expect(reason).toContain("couldn't check the graphics adapters");
    expect(reason).toContain('class key: access denied');
  });

  it('a rejected probe call reads the same way', async () => {
    gpuCapability.mockRejectedValue(new Error('ipc channel closed'));
    await mount();
    await flush();
    const reason = screen.getByTestId('gpu-reason').textContent;
    expect(reason).toContain("couldn't check");
    expect(reason).toContain('ipc channel closed');
  });

  it('a stored choice stays visible while the control is hidden, and Reset saves Automatic', async () => {
    general.gpu_preference = 'high_performance';
    gpuCapability.mockResolvedValue({
      status: 'ok',
      data: status('windows_registry', { kind: 'single_gpu' }),
    });
    await mount();
    await flush();
    expect(screen.getByTestId('gpu-stored').textContent).toContain(
      'still written to the Windows graphics preference',
    );
    await fireEvent.click(screen.getByTestId('gpu-reset'));
    await flush();
    await flush();
    expect(commands.appSettingsPatchGeneral).toHaveBeenCalledWith({ gpu_preference: 'auto' });
    expect(screen.queryByTestId('gpu-stored')).toBeNull();
  });

  it.each([
    ['none', 'power_saving'],
    ['linux_env', 'power_saving'],
  ] as const)('on %s a stored %s says it has no effect', async (mechanism, pref) => {
    general.gpu_preference = pref;
    gpuCapability.mockResolvedValue({
      status: 'ok',
      data: status(mechanism, { kind: 'unsupported' }),
    });
    await mount();
    await flush();
    expect(screen.getByTestId('gpu-stored').textContent).toContain('has no effect on this system');
  });

  it('nothing is claimed while checking, and nothing on Automatic', async () => {
    general.gpu_preference = 'auto';
    gpuCapability.mockResolvedValue({
      status: 'ok',
      data: status('windows_registry', { kind: 'single_gpu' }),
    });
    await mount();
    await flush();
    expect(screen.queryByTestId('gpu-stored')).toBeNull();
    expect(screen.queryByTestId('gpu-reset')).toBeNull();
  });

  it('the GPU select is described by the mechanism note', async () => {
    await mount();
    const select = await screen.findByTestId('gpu-select');
    expect(describedText(select)).toContain('UserGpuPreferences');
  });
});
