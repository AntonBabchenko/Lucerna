import { render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { GpuCapability } from '$lib/ipc/bindings';

// Hoisted so they're available before vi.mock factory runs.
const { gpuCapability } = vi.hoisted(() => ({
  gpuCapability: vi.fn(
    async (): Promise<{ status: 'ok'; data: GpuCapability }> => ({
      status: 'ok',
      data: { kind: 'unsupported' },
    }),
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
          gpu_preference: 'auto',
        },
      },
    })),
    appSettingsSetGeneral: vi.fn(async () => ({ status: 'ok', data: null })),
    updateCheck: vi.fn(async () => ({
      status: 'ok',
      data: { available: false, current: '0.0.0', latest: '0.0.0' },
    })),
    gpuCapability: () => gpuCapability(),
  },
}));

import GamePanel from '$lib/settings/GamePanel.svelte';

describe('GamePanel GPU dropdown', () => {
  it('shows the gpu-select when capability is "available"', async () => {
    const available: GpuCapability = {
      kind: 'available',
      gpus: [{ name: 'NVIDIA' }, { name: 'Intel' }],
      high: 'NVIDIA',
      low: 'Intel',
    };
    gpuCapability.mockResolvedValueOnce({ status: 'ok', data: available });

    const { findByTestId } = render(GamePanel);
    const select = await findByTestId('gpu-select');
    expect(select).toBeTruthy();
  });

  it('does not show gpu-select when capability is "single_gpu"', async () => {
    const singleGpu: GpuCapability = { kind: 'single_gpu' };
    gpuCapability.mockResolvedValueOnce({ status: 'ok', data: singleGpu });

    const { queryByTestId } = render(GamePanel);
    // Wait for onMount to complete.
    await new Promise((r) => setTimeout(r, 0));
    expect(queryByTestId('gpu-select')).toBeNull();
  });

  it('does not show gpu-select when capability is "unsupported"', async () => {
    const unsupported: GpuCapability = { kind: 'unsupported' };
    gpuCapability.mockResolvedValueOnce({ status: 'ok', data: unsupported });

    const { queryByTestId } = render(GamePanel);
    await new Promise((r) => setTimeout(r, 0));
    expect(queryByTestId('gpu-select')).toBeNull();
  });
});
