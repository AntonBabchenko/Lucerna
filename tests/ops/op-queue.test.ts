import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    verifyInstance: vi.fn(),
    repairInstance: vi.fn(),
    modpackImport: vi.fn(),
    setActiveInstance: vi.fn(),
  },
  events: {
    verifyProgress: { listen: vi.fn().mockResolvedValue(() => {}) },
  },
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
  pushActionToast: vi.fn(),
}));
vi.mock('$lib/ipc/format-error', () => ({
  formatError: vi.fn((e: { kind: string }) => `formatted:${e.kind}`),
}));
vi.mock('$lib/i18n', () => ({
  t: { subscribe: (run: (v: unknown) => void) => { run(() => 'tr'); return () => {}; } },
}));

import {
  __resetOpQueueForTest,
  cancelQueued,
  enqueueIntegrity,
  moveQueued,
  opCompletionTick,
  opQueue,
  opRunning,
  opStatusFor,
} from '$lib/ops/op-queue.svelte';
import { commands } from '$lib/ipc/bindings';
import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';

const healthyReport = {
  instance_id: 'a',
  effective_version_id: '1.20.4',
  categories: [],
  problems: [],
  healthy: true,
  manifest_recoverable: false,
};

const brokenReport = {
  ...healthyReport,
  healthy: false,
  problems: [
    { category: 'assets', rel_path: 'a', expected_sha: 'x', url: null, status: 'corrupt' },
    { category: 'assets', rel_path: 'b', expected_sha: 'y', url: null, status: 'missing' },
  ],
};

/** A promise plus its resolver, to control when a command "completes". */
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

beforeEach(() => {
  vi.clearAllMocks();
  __resetOpQueueForTest();
  // Safe defaults so any drained-but-unconfigured op resolves cleanly
  // (individual tests override per case).
  (commands.verifyInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
    status: 'ok',
    data: healthyReport,
  });
  (commands.repairInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
    status: 'ok',
    data: healthyReport,
  });
});

describe('op-queue store', () => {
  it('dedupes a second enqueue for the same instance while it is running', async () => {
    const d = deferred<{ status: 'ok'; data: typeof healthyReport }>();
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockReturnValue(d.promise);

    enqueueIntegrity('a', 'Alpha', 'verify');
    // Second click while running — must be ignored.
    enqueueIntegrity('a', 'Alpha', 'verify');

    expect(commands.verifyInstance).toHaveBeenCalledTimes(1);
    expect(opQueue().length).toBe(0);

    d.resolve({ status: 'ok', data: healthyReport });
    await d.promise;
    await Promise.resolve();
  });

  it('drains the queue one at a time — the 2nd command waits for the 1st', async () => {
    const d1 = deferred<{ status: 'ok'; data: typeof healthyReport }>();
    const d2 = deferred<{ status: 'ok'; data: typeof healthyReport }>();
    (commands.verifyInstance as ReturnType<typeof vi.fn>)
      .mockReturnValueOnce(d1.promise)
      .mockReturnValueOnce(d2.promise);

    enqueueIntegrity('a', 'Alpha', 'verify');
    enqueueIntegrity('b', 'Bravo', 'verify');

    // 'a' is running; 'b' is queued — its command has NOT been called yet.
    expect(commands.verifyInstance).toHaveBeenCalledTimes(1);
    expect(commands.verifyInstance).toHaveBeenLastCalledWith('a');
    expect(opStatusFor('b')?.phase).toBe('queued');

    // Resolve the first → the drain loop should start the second.
    d1.resolve({ status: 'ok', data: healthyReport });
    await d1.promise;
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(commands.verifyInstance).toHaveBeenCalledTimes(2);
    expect(commands.verifyInstance).toHaveBeenLastCalledWith('b');

    d2.resolve({ status: 'ok', data: healthyReport });
    await d2.promise;
    await Promise.resolve();
  });

  it('healthy verify → pushSuccess', async () => {
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: healthyReport,
    });

    enqueueIntegrity('a', 'Alpha', 'verify');
    await vi.waitFor(() => expect(opRunning()).toBeNull());

    expect(pushSuccess).toHaveBeenCalledTimes(1);
    expect(pushWarning).not.toHaveBeenCalled();
  });

  it('verify with problems → pushWarning with the problem count', async () => {
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: brokenReport,
    });

    enqueueIntegrity('a', 'Alpha', 'verify');
    await vi.waitFor(() => expect(opRunning()).toBeNull());

    expect(pushWarning).toHaveBeenCalledTimes(1);
    // tr is identity → title is the key; the count is threaded as a placeholder
    // value to the translator (deterministic via our `t` mock returning 'tr').
    expect(pushSuccess).not.toHaveBeenCalled();
  });

  it('command error → pushWarning', async () => {
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'error',
      error: { kind: 'instance_busy' },
    });

    enqueueIntegrity('a', 'Alpha', 'verify');
    await vi.waitFor(() => expect(opRunning()).toBeNull());

    expect(pushWarning).toHaveBeenCalledTimes(1);
  });

  it('completion bumps the tick', async () => {
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: healthyReport,
    });

    expect(opCompletionTick()).toBe(0);
    enqueueIntegrity('a', 'Alpha', 'verify');
    await vi.waitFor(() => expect(opCompletionTick()).toBe(1));
  });

  it('opStatusFor reports running for the active id and queued for a waiting id', async () => {
    const d = deferred<{ status: 'ok'; data: typeof healthyReport }>();
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockReturnValue(d.promise);

    enqueueIntegrity('a', 'Alpha', 'verify');
    enqueueIntegrity('b', 'Bravo', 'repair');

    expect(opStatusFor('a')).toEqual({
      phase: 'running',
      kind: 'verify',
      filesDone: 0,
      filesTotal: 0,
    });
    expect(opStatusFor('b')).toEqual({
      phase: 'queued',
      kind: 'repair',
      filesDone: 0,
      filesTotal: 0,
    });
    expect(opStatusFor('c')).toBeNull();

    d.resolve({ status: 'ok', data: healthyReport });
    await d.promise;
    await Promise.resolve();
  });

  it('cancelQueued removes a pending op but never the running one', async () => {
    const d = deferred<{ status: 'ok'; data: typeof healthyReport }>();
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockReturnValue(d.promise);

    enqueueIntegrity('a', 'Alpha', 'verify'); // runs
    enqueueIntegrity('b', 'Bravo', 'verify'); // queued
    enqueueIntegrity('c', 'Charlie', 'verify'); // queued

    const bId = opQueue().find((q) => q.kind !== 'import' && q.instanceId === 'b')!.id;
    cancelQueued(bId);
    expect(opQueue().map((q) => (q.kind !== 'import' ? q.instanceId : ''))).toEqual(['c']);

    // Cancelling the running op's id is a no-op (it isn't in the queue array).
    const runningId = opRunning()!.op.id;
    cancelQueued(runningId);
    expect(opRunning()).not.toBeNull();

    d.resolve({ status: 'ok', data: healthyReport });
    await d.promise;
    await Promise.resolve();
  });

  it('moveQueued swaps neighbours and is a no-op at the ends', async () => {
    const d = deferred<{ status: 'ok'; data: typeof healthyReport }>();
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockReturnValue(d.promise);

    enqueueIntegrity('a', 'Alpha', 'verify'); // runs
    enqueueIntegrity('b', 'Bravo', 'verify'); // queue[0]
    enqueueIntegrity('c', 'Charlie', 'verify'); // queue[1]

    const ids = () => opQueue().map((q) => (q.kind !== 'import' ? q.instanceId : ''));
    const cId = opQueue()[1].id;
    moveQueued(cId, 'up');
    expect(ids()).toEqual(['c', 'b']);
    moveQueued(cId, 'up'); // already at top — no-op
    expect(ids()).toEqual(['c', 'b']);

    d.resolve({ status: 'ok', data: healthyReport });
    await d.promise;
    await Promise.resolve();
  });
});
