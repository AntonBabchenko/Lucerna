import { beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({ updateDismiss: vi.fn() }));
vi.mock('$lib/ipc/bindings', async (orig) => {
  const real = await orig<typeof import('$lib/ipc/bindings')>();
  return {
    ...real,
    commands: { ...real.commands, updateDismiss: (v: string) => h.updateDismiss(v) },
  };
});

import type { UpdateInfo } from '$lib/ipc/bindings';
import { dismiss, pushActionToast, toastList } from '$lib/toasts/toasts.svelte';
import { showUpdateToast, UPDATE_TOAST_TTL_MS } from '$lib/update/state.svelte';

const INFO = {
  current: '0.24.0',
  latest: '0.25.0',
  available: true,
  release_url: 'https://github.com/AntonBabchenko/Lucerna/releases/tag/v0.25.0',
  installer: null,
  sha256sums: null,
  cosign_bundle: null,
} as unknown as UpdateInfo;

describe('action toast', () => {
  beforeEach(() => {
    for (const t of [...toastList()]) dismiss(t.id);
  });

  it('stores an action with label and run callback', () => {
    let ran = 0;
    const id = pushActionToast('info', 'Update available', { label: 'Update', run: () => ran++ });
    const t = toastList().find((x) => x.id === id)!;
    expect(t.action?.label).toBe('Update');
    t.action!.run();
    expect(ran).toBe(1);
  });

  it('action toast is sticky (info/warning do not auto-dismiss)', () => {
    const id = pushActionToast('warning', 'Verify failed', { label: 'Open', run: () => {} });
    expect(toastList().some((x) => x.id === id)).toBe(true);
  });
});

describe('the startup update notice', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    for (const t of [...toastList()]) dismiss(t.id);
    h.updateDismiss.mockResolvedValue({ status: 'ok', data: '0.25.0' });
  });

  it('offers Skip this version as a readable action', () => {
    const id = showUpdateToast(INFO);
    const t = toastList().find((x) => x.id === id)!;
    expect(t.secondary?.label).toBe('Skip this version');
  });

  it('skips only when Skip is chosen', async () => {
    const id = showUpdateToast(INFO);
    toastList()
      .find((x) => x.id === id)!
      .secondary!.run();
    await vi.waitFor(() => expect(h.updateDismiss).toHaveBeenCalledWith('0.25.0'));
  });

  it('closing it skips nothing — it comes back next launch', () => {
    const id = showUpdateToast(INFO);
    dismiss(id);
    expect(h.updateDismiss).not.toHaveBeenCalled();
  });

  it('hides itself after its time without skipping', () => {
    vi.useFakeTimers();
    try {
      showUpdateToast(INFO);
      vi.advanceTimersByTime(UPDATE_TOAST_TTL_MS + 1);
      expect(toastList()).toHaveLength(0);
      expect(h.updateDismiss).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
});
