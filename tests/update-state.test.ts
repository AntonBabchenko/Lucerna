import { beforeEach, describe, expect, it, vi } from 'vitest';

// Typed wide enough for both outcomes (svelte-check type-checks tests/ too).
const updateDismissMock = vi.fn(
  async (
    _v: string,
  ): Promise<
    | { status: 'ok'; data: string | null }
    | { status: 'error'; error: { kind: 'io'; path: string; details: string } }
  > => ({ status: 'ok', data: '0.9.1' }),
);
const updateClearDismissedMock = vi.fn(async () => ({ status: 'ok' as const, data: null }));
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    updateInstall: vi.fn(async () => ({ status: 'ok', data: null })),
    updateDismiss: (v: string) => updateDismissMock(v),
    updateClearDismissed: () => updateClearDismissedMock(),
  },
}));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn(async () => {}) }));

// The module imports these toast helpers at load time; the factory must supply
// all of them or the import is undefined at call time.
const pushWarningMock = vi.fn();
vi.mock('$lib/toasts/toasts.svelte', () => ({
  dismiss: vi.fn(),
  pushActionToast: vi.fn(),
  pushInfo: vi.fn(),
  pushProgress: vi.fn(() => 1),
  pushWarning: (...a: unknown[]) => pushWarningMock(...a),
  updateToast: vi.fn(),
  updateToastProgress: vi.fn(),
}));

import { skipUpdate, stopSkipping, updateState } from '$lib/update/state.svelte';

describe('skipping a version', () => {
  beforeEach(() => {
    updateState.value = null;
    vi.clearAllMocks();
  });

  it('persists the version and returns the skip the backend now holds', async () => {
    expect(await skipUpdate('0.9.1')).toEqual({ ok: true, skipped: '0.9.1' });
    expect(updateDismissMock).toHaveBeenCalledWith('0.9.1');
  });

  it('keeps the offer: a skipped version is still available, only the startup notice stops', async () => {
    const info = { latest: '0.9.1', available: true } as never;
    updateState.value = info;
    await skipUpdate('0.9.1');
    // toEqual, not toBe: the rune hands back a reactive proxy.
    expect(updateState.value).toEqual(info);
  });

  it('reports a skip that did not persist instead of claiming it', async () => {
    updateDismissMock.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: '<app.json>', details: 'disk full' },
    });
    const r = await skipUpdate('0.9.1');
    expect(r.ok).toBe(false);
  });

  it('stop skipping clears it and returns what the backend now holds', async () => {
    expect(await stopSkipping()).toEqual({ ok: true, skipped: null });
    expect(updateClearDismissedMock).toHaveBeenCalledTimes(1);
  });
});
