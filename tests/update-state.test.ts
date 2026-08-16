import { beforeEach, describe, expect, it, vi } from 'vitest';

const updateDismissMock = vi.fn(async () => ({ status: 'ok', data: null }));
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    updateInstall: vi.fn(async () => ({ status: 'ok', data: null })),
    updateDismiss: (...a: unknown[]) => updateDismissMock(...(a as [])),
  },
}));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn(async () => {}) }));

// The module imports five toast helpers at load time; the factory must supply
// all of them or the import is undefined at call time.
const pushWarningMock = vi.fn();
vi.mock('$lib/toasts/toasts.svelte', () => ({
  dismiss: vi.fn(),
  pushActionToast: vi.fn(),
  pushProgress: vi.fn(() => 1),
  pushWarning: (...a: unknown[]) => pushWarningMock(...a),
  updateToastProgress: vi.fn(),
}));

import { dismissUpdate, updateState } from '$lib/update/state.svelte';

describe('update state', () => {
  beforeEach(() => {
    updateState.value = null;
    vi.clearAllMocks();
  });

  it('dismissUpdate clears the rune and persists the version', async () => {
    updateState.value = { latest: '0.9.1' } as never;
    await dismissUpdate('0.9.1');
    expect(updateDismissMock).toHaveBeenCalledWith('0.9.1');
    expect(updateState.value).toBeNull();
  });

  it('keeps the update visible and reports when the dismiss does not persist', async () => {
    const info = { latest: '0.9.1' } as never;
    updateState.value = info;
    updateDismissMock.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: '<app.json>', details: 'disk full' },
    } as never);

    await dismissUpdate('0.9.1');

    // Settings → Updates must keep reporting an update that is still available
    // AND still undismissed — clearing it would be a claim we can't back.
    expect(updateState.value).toBe(info);
    expect(pushWarningMock).toHaveBeenCalledTimes(1);
  });
});
