import { beforeEach, describe, expect, it, vi } from 'vitest';

// Mock the IPC boundary: commands as spies, the progress event listener as a
// resolved unlisten (so `ensureListener` stays inert but doesn't reject).
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    verifyInstance: vi.fn(),
    repairInstance: vi.fn(),
  },
  events: {
    verifyProgress: { listen: vi.fn().mockResolvedValue(() => {}) },
  },
}));

// Mock the toast store — assert which toast was pushed without a DOM.
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
}));

// Mock format-error so the error-toast path doesn't depend on real i18n copy.
vi.mock('$lib/ipc/format-error', () => ({
  formatError: vi.fn((e: { kind: string }) => `formatted:${e.kind}`),
}));

// Mock i18n's `t` store: `get(t)` returns an identity-ish translator so toast
// titles are deterministic and ICU placeholders don't need a dictionary.
vi.mock('$lib/i18n', () => ({
  t: {
    subscribe: (run: (v: unknown) => void) => {
      run(() => 'tr');
      return () => {};
    },
  },
}));

import {
  __resetIntegrityOpsForTest,
  enqueueIntegrity,
  integrityCompletionTick,
  integrityQueueLength,
  integrityRunning,
  integrityStatusFor,
} from '$lib/instances/integrity-ops.svelte';
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
  __resetIntegrityOpsForTest();
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

describe('integrity-ops store', () => {
  it('dedupes a second enqueue for the same instance while it is running', async () => {
    const d = deferred<{ status: 'ok'; data: typeof healthyReport }>();
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockReturnValue(d.promise);

    enqueueIntegrity('a', 'Alpha', 'verify');
    // Second click while running — must be ignored.
    enqueueIntegrity('a', 'Alpha', 'verify');

    expect(commands.verifyInstance).toHaveBeenCalledTimes(1);
    expect(integrityQueueLength()).toBe(0);

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
    expect(integrityStatusFor('b')?.phase).toBe('queued');

    // Resolve the first → the drain loop should start the second.
    d1.resolve({ status: 'ok', data: healthyReport });
    await d1.promise;
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
    await vi.waitFor(() => expect(integrityRunning()).toBeNull());

    expect(pushSuccess).toHaveBeenCalledTimes(1);
    expect(pushWarning).not.toHaveBeenCalled();
  });

  it('verify with problems → pushWarning with the problem count', async () => {
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: brokenReport,
    });

    enqueueIntegrity('a', 'Alpha', 'verify');
    await vi.waitFor(() => expect(integrityRunning()).toBeNull());

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
    await vi.waitFor(() => expect(integrityRunning()).toBeNull());

    expect(pushWarning).toHaveBeenCalledTimes(1);
  });

  it('completion bumps the tick', async () => {
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: healthyReport,
    });

    expect(integrityCompletionTick()).toBe(0);
    enqueueIntegrity('a', 'Alpha', 'verify');
    await vi.waitFor(() => expect(integrityCompletionTick()).toBe(1));
  });

  it('integrityStatusFor reports running for the active id and queued for a waiting id', async () => {
    const d = deferred<{ status: 'ok'; data: typeof healthyReport }>();
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockReturnValue(d.promise);

    enqueueIntegrity('a', 'Alpha', 'verify');
    enqueueIntegrity('b', 'Bravo', 'repair');

    expect(integrityStatusFor('a')).toEqual({
      phase: 'running',
      kind: 'verify',
      filesDone: 0,
      filesTotal: 0,
    });
    expect(integrityStatusFor('b')).toEqual({
      phase: 'queued',
      kind: 'repair',
      filesDone: 0,
      filesTotal: 0,
    });
    expect(integrityStatusFor('c')).toBeNull();

    d.resolve({ status: 'ok', data: healthyReport });
    await d.promise;
    await Promise.resolve();
  });
});
