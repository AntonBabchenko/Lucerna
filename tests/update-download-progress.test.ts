import { beforeEach, describe, expect, it, type Mock, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: { updateInstall: vi.fn() },
  events: { downloadProgress: { listen: vi.fn() } },
}));
vi.mock('$lib/ipc/format-error', () => ({
  formatError: (e: { kind: string }) => `err:${e.kind}`,
}));
// get(t) reads the store value once; expose a translate fn that echoes the key.
vi.mock('$lib/i18n', () => ({
  t: {
    subscribe: (fn: (v: (k: string) => string) => void) => {
      fn((k: string) => k);
      return () => {};
    },
  },
}));

import { commands, events } from '$lib/ipc/bindings';
import { dismiss, toastList } from '$lib/toasts/toasts.svelte';
import { runUpdate, updateInstalling, updateState } from '$lib/update/state.svelte';

const INSTALLER_URL = 'https://cdn.example/Lucerna_x64-setup.exe';

function progressToast() {
  return toastList().find((t) => t.progress !== undefined);
}

async function flush() {
  await Promise.resolve();
  await Promise.resolve();
}

describe('update download progress', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    updateInstalling.value = false;
    for (const t of [...toastList()]) dismiss(t.id);
    updateState.value = { installer: { url: INSTALLER_URL } } as never;
  });

  it('drives the toast from installer events, ignores other urls and null totals', async () => {
    let cb!: (e: {
      payload: { url: string; bytes_done: number; bytes_total: number | null };
    }) => void;
    (events.downloadProgress.listen as Mock).mockImplementation((c) => {
      cb = c;
      return Promise.resolve(() => {});
    });
    let resolveInstall!: (v: unknown) => void;
    (commands.updateInstall as Mock).mockReturnValue(
      new Promise((res) => {
        resolveInstall = res;
      }),
    );

    const p = runUpdate();
    await flush();

    cb({ payload: { url: INSTALLER_URL, bytes_done: 50, bytes_total: 100 } });
    expect(progressToast()?.progress).toBe(0.5);

    // a different download's progress must not move the update bar
    cb({ payload: { url: 'https://cdn.example/other.bin', bytes_done: 999, bytes_total: 1000 } });
    expect(progressToast()?.progress).toBe(0.5);

    // no Content-Length => indeterminate (null), never NaN
    cb({ payload: { url: INSTALLER_URL, bytes_done: 10, bytes_total: null } });
    expect(progressToast()?.progress).toBeNull();

    resolveInstall({ status: 'ok', data: null });
    await p;

    // the progress toast is dismissed after the install settles
    expect(progressToast()).toBeUndefined();
  });

  it('tears down the listener and shows a warning on failure', async () => {
    const unlisten = vi.fn();
    (events.downloadProgress.listen as Mock).mockResolvedValue(unlisten);
    (commands.updateInstall as Mock).mockResolvedValue({
      status: 'error',
      error: { kind: 'update_verification_failed' },
    });

    await runUpdate();

    expect(unlisten).toHaveBeenCalledTimes(1);
    expect(progressToast()).toBeUndefined();
    expect(toastList().some((t) => t.kind === 'warning')).toBe(true);
    expect(updateInstalling.value).toBe(false);
  });

  it('recovers if the progress listener fails to register (no leak, no soft-lock)', async () => {
    (events.downloadProgress.listen as Mock).mockRejectedValue(new Error('ipc listen failed'));
    (commands.updateInstall as Mock).mockResolvedValue({ status: 'ok', data: null });

    await runUpdate();

    expect(progressToast()).toBeUndefined();
    expect(toastList().some((t) => t.kind === 'warning')).toBe(true);
    expect(updateInstalling.value).toBe(false);
  });
});
