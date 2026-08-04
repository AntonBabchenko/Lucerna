// Regression coverage for the "cancelling a queued op must not toast a
// failure" bug: `2900e11c` made the serial lane genuinely gate execution —
// `cancelQueued` rejects a queued task's gate with `TaskCancelledError`
// before its backend command ever fires — but every serial adapter's catch
// block treated that rejection exactly like a real failure, so op-queue's
// generic error branch (`pushWarning`) fired for something the user chose
// to do.
//
// Deliberately does NOT mock `$lib/toasts/toasts.svelte` (unlike
// tests/ops/op-queue.test.ts) — inspecting the REAL `toastList()` (see
// tests/toasts-store.test.ts) proves no warning toast reaches the actual
// surface, not merely that a mock function wasn't called.

import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: ((m: unknown) => void) | null = null;
  },
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    cloneInstance: vi.fn(),
    setActiveInstance: vi.fn(),
  },
}));

vi.mock('$lib/i18n', () => ({
  t: {
    subscribe: (run: (v: unknown) => void) => {
      // Identity translator — a toast's title/lines resolve to the i18n key.
      run((key: string) => key);
      return () => {};
    },
  },
}));

import { commands } from '$lib/ipc/bindings';
import { __resetOpQueueForTest, enqueueClone, opCompletionTick } from '$lib/ops/op-queue.svelte';
import { __resetTasksForTest, cancelQueued, taskFor } from '$lib/tasks/registry.svelte';
import { dismiss, toastList } from '$lib/toasts/toasts.svelte';

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

const cloneReq = (sourceId: string) => ({
  sourceId,
  newName: 'Copy',
  options: {
    saves: true,
    settings: true,
    packs: true,
    config: true,
    options_txt: true,
    playtime: true,
  },
});

describe('op-queue cancellation toast', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    __resetOpQueueForTest();
    __resetTasksForTest();
    for (const toast of [...toastList()]) dismiss(toast.id);
  });

  it('cancelling a queued clone produces no warning toast', async () => {
    const first = deferred<{ status: 'ok'; data: { id: string; name: string } }>();
    vi.mocked(commands.cloneInstance).mockReturnValueOnce(first.promise as never);

    enqueueClone('First', cloneReq('inst-1'));
    // Different source instance → not deduped, queues behind inst-1 in the
    // shared serial lane (see op-queue.svelte.ts's module doc comment).
    enqueueClone('Second', cloneReq('inst-2'));

    const queued = await vi.waitFor(() => {
      const t = taskFor({ instanceId: 'inst-2' });
      expect(t?.state).toBe('queued');
      return t!;
    });
    cancelQueued(queued.id);

    first.resolve({ status: 'ok', data: { id: 'clone-1', name: 'First' } });
    await first.promise;

    // Both ops must settle — inst-1 for real, inst-2 via cancellation —
    // before the toast stack can be asserted on.
    await vi.waitFor(() => expect(opCompletionTick()).toBe(2));

    // inst-2's command must never have fired (the actual gating fix from
    // 2900e11c — reasserted here as a precondition, not the bug under test).
    expect(commands.cloneInstance).toHaveBeenCalledTimes(1);

    // The bug: this used to contain a warning toast for the cancelled inst-2
    // clone ("instance.clone.failed").
    expect(toastList().some((toast) => toast.kind === 'warning')).toBe(false);
  });
});
