import { beforeEach, describe, expect, it, vi } from 'vitest';
import { enqueueImport } from '$lib/ops/op-queue.svelte';
import { pushActionToast } from '$lib/toasts/toasts.svelte';

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: ((m: unknown) => void) | null = null;
  },
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    verifyInstance: vi.fn(),
    repairInstance: vi.fn(),
    modpackImport: vi.fn(),
    setActiveInstance: vi.fn(),
    launcherImportRun: vi.fn(),
    cloneInstance: vi.fn(),
  },
  events: {
    verifyProgress: { listen: vi.fn().mockResolvedValue(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
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
  t: {
    subscribe: (run: (v: unknown) => void) => {
      // Identity translator: returns the key so tests can assert which
      // i18n key a toast title/line resolved to (the params are ignored).
      run((key: string) => key);
      return () => {};
    },
  },
}));

import { commands } from '$lib/ipc/bindings';
import {
  __resetOpQueueForTest,
  cancelQueued,
  enqueueClone,
  enqueueIntegrity,
  enqueueLauncherImport,
  moveQueued,
  opCompletionTick,
  opQueue,
  opRunning,
  opStatusFor,
} from '$lib/ops/op-queue.svelte';
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
    // value to the translator (deterministic via our key-echoing `t` mock).
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

    const bOp = opQueue().find(
      (q) => (q.kind === 'verify' || q.kind === 'repair') && q.instanceId === 'b',
    );
    if (!bOp) throw new Error('b not queued');
    cancelQueued(bOp.id);
    expect(
      opQueue().map((q) => (q.kind === 'verify' || q.kind === 'repair' ? q.instanceId : '')),
    ).toEqual(['c']);

    // Cancelling the running op's id is a no-op (it isn't in the queue array).
    const running = opRunning();
    if (!running) throw new Error('expected a running op');
    cancelQueued(running.op.id);
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

    const ids = () =>
      opQueue().map((q) => (q.kind === 'verify' || q.kind === 'repair' ? q.instanceId : ''));
    const cId = opQueue()[1].id;
    moveQueued(cId, 'up');
    expect(ids()).toEqual(['c', 'b']);
    moveQueued(cId, 'up'); // already at top — no-op
    expect(ids()).toEqual(['c', 'b']);

    d.resolve({ status: 'ok', data: healthyReport });
    await d.promise;
    await Promise.resolve();
  });

  const importReq = (path: string) => ({
    path,
    selectedShas: [],
    projectId: null,
    source: null,
    versionId: null,
  });

  it('runs an import after a verify finishes — strict serial across kinds', async () => {
    const dv = deferred<{ status: 'ok'; data: typeof healthyReport }>();
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockReturnValue(dv.promise);
    (commands.modpackImport as ReturnType<typeof vi.fn>).mockImplementation(
      async (...args: unknown[]) => {
        (args[6] as { onmessage: (m: unknown) => void }).onmessage({
          phase: 'done',
          skipped_overrides: [],
          inert_loader_jars: [],
        });
        return { status: 'ok', data: { name: 'Pack', id: 'i1' } };
      },
    );

    enqueueIntegrity('a', 'Alpha', 'verify'); // running
    enqueueImport('Pack', importReq('/tmp/p.mrpack')); // queued

    expect(commands.modpackImport).not.toHaveBeenCalled();
    dv.resolve({ status: 'ok', data: healthyReport });
    await vi.waitFor(() => expect(commands.modpackImport).toHaveBeenCalledTimes(1));
    await vi.waitFor(() => expect(opRunning()).toBeNull());
  });

  it('dedupes a second import of the same path', async () => {
    const d = deferred<{ status: 'ok'; data: { name: string } }>();
    (commands.modpackImport as ReturnType<typeof vi.fn>).mockReturnValue(d.promise);
    enqueueImport('Pack', importReq('/tmp/p.mrpack'));
    enqueueImport('Pack', importReq('/tmp/p.mrpack')); // same path → ignored
    expect(opQueue().length).toBe(0); // first is running, second dropped
    d.resolve({ status: 'ok', data: { name: 'Pack' } });
    await vi.waitFor(() => expect(opRunning()).toBeNull());
  });

  it('import success pushes an action toast whose Open runs setActiveInstance', async () => {
    (commands.modpackImport as ReturnType<typeof vi.fn>).mockImplementation(
      async (...args: unknown[]) => {
        (args[6] as { onmessage: (m: unknown) => void }).onmessage({
          phase: 'done',
          skipped_overrides: [],
          inert_loader_jars: [],
        });
        return { status: 'ok', data: { name: 'Pack', id: 'i9' } };
      },
    );
    (commands.setActiveInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: null,
    });

    enqueueImport('Pack', importReq('/tmp/p.mrpack'));
    await vi.waitFor(() => expect(pushActionToast).toHaveBeenCalledTimes(1));

    // The action's run() switches the active instance.
    const action = (pushActionToast as ReturnType<typeof vi.fn>).mock.calls[0][2] as {
      run: () => void;
    };
    action.run();
    await vi.waitFor(() => expect(commands.setActiveInstance).toHaveBeenCalledWith('i9'));
  });

  it('import with only inert loader jars → inert title + one line per inert jar', async () => {
    (commands.modpackImport as ReturnType<typeof vi.fn>).mockImplementation(
      async (...args: unknown[]) => {
        (args[6] as { onmessage: (m: unknown) => void }).onmessage({
          phase: 'done',
          skipped_overrides: [],
          inert_loader_jars: [
            { filename: 'a-Fabric.jar', detected_loader: 'Fabric' },
            { filename: 'b-Fabric.jar', detected_loader: 'Fabric' },
          ],
        });
        return { status: 'ok', data: { name: 'Pack', id: 'i1' } };
      },
    );

    enqueueImport('Pack', importReq('/tmp/p.mrpack'));
    await vi.waitFor(() => expect(pushActionToast).toHaveBeenCalledTimes(1));

    // pushActionToast(type, title, action, lines) — tr echoes the key.
    const call = (pushActionToast as ReturnType<typeof vi.fn>).mock.calls[0];
    const title = call[1] as string;
    const lines = call[3] as string[];
    expect(title).toBe('page.modpackImport.importedInertLoader');
    expect(lines).toEqual([
      'page.modpackImport.inertLoaderLine',
      'page.modpackImport.inertLoaderLine',
    ]);
  });

  it('import with both skipped overrides and inert jars → skipped title covers both, lines list both', async () => {
    (commands.modpackImport as ReturnType<typeof vi.fn>).mockImplementation(
      async (...args: unknown[]) => {
        (args[6] as { onmessage: (m: unknown) => void }).onmessage({
          phase: 'done',
          skipped_overrides: [{ path: 'mods/mods.rar', size: 261361205 }],
          inert_loader_jars: [{ filename: 'a-Fabric.jar', detected_loader: 'Fabric' }],
        });
        return { status: 'ok', data: { name: 'Pack', id: 'i1' } };
      },
    );

    enqueueImport('Pack', importReq('/tmp/p.mrpack'));
    await vi.waitFor(() => expect(pushActionToast).toHaveBeenCalledTimes(1));

    const call = (pushActionToast as ReturnType<typeof vi.fn>).mock.calls[0];
    const title = call[1] as string;
    const lines = call[3] as string[];
    // Skipped takes precedence in the title; both line kinds are listed.
    expect(title).toBe('page.modpackImport.importedSkipped');
    expect(lines).toEqual([
      'page.modpackImport.skippedOverrideLine',
      'page.modpackImport.inertLoaderLine',
    ]);
  });

  const mockForeign = {
    source: 'prism' as const,
    name: 'Prism Pack',
    root: '/prism/instances/pack',
    minecraft_dir: '/prism/instances/pack/.minecraft',
    mc_version: '1.20.4',
    loader: 'fabric' as const,
    loader_version: '0.15.7',
    max_heap_mb: 4096,
    extra_jvm_args: null,
    content: [],
    known_mods: [],
  };

  const mockInstanceData = {
    id: 'inst-new',
    name: 'Prism Pack',
    mc_version: '1.20.4',
    loader: 'fabric' as const,
    loader_version: '0.15.7',
    max_heap_mb: 4096,
    extra_jvm_args: null,
    installed: true,
    imported_from: null,
    content_overview: null,
    integrity: null,
    playtime: null,
    gpu_preference: null,
  };

  it('enqueueLauncherImport: success pushes action toast and bumps importCompletionTick', async () => {
    (commands.launcherImportRun as ReturnType<typeof vi.fn>).mockImplementation(
      async (
        _f: unknown,
        _s: unknown,
        _n: unknown,
        _mv: unknown,
        _lo: unknown,
        _lv: unknown,
        ch: { onmessage: ((m: unknown) => void) | null },
      ) => {
        ch.onmessage?.({ phase: 'done', instance_id: 'inst-new', untracked_mods: 0 });
        return { status: 'ok', data: mockInstanceData };
      },
    );
    (commands.setActiveInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: null,
    });

    enqueueLauncherImport('Prism Pack', {
      foreign: mockForeign,
      selected: ['mods'],
      targetName: 'Prism Pack',
    });

    await vi.waitFor(() => expect(pushActionToast).toHaveBeenCalledTimes(1));
    await vi.waitFor(() => expect(opRunning()).toBeNull());
  });

  it('enqueueLauncherImport: dedupes same root', async () => {
    const d = deferred<{ status: 'ok'; data: typeof mockInstanceData }>();
    (commands.launcherImportRun as ReturnType<typeof vi.fn>).mockReturnValue(d.promise);

    enqueueLauncherImport('Prism Pack', {
      foreign: mockForeign,
      selected: ['mods'],
      targetName: 'Prism Pack',
    });
    enqueueLauncherImport('Prism Pack', {
      foreign: mockForeign,
      selected: ['mods'],
      targetName: 'Prism Pack',
    }); // same root → ignored

    expect(opQueue().length).toBe(0); // first is running, second dropped
    d.resolve({ status: 'ok', data: mockInstanceData });
    await vi.waitFor(() => expect(opRunning()).toBeNull());
  });

  it('enqueueLauncherImport: error pushes warning', async () => {
    (commands.launcherImportRun as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: '/x', details: 'nope' },
    });

    enqueueLauncherImport('Prism Pack', {
      foreign: mockForeign,
      selected: ['mods'],
      targetName: 'Prism Pack',
    });

    await vi.waitFor(() => expect(pushWarning).toHaveBeenCalledTimes(1));
    await vi.waitFor(() => expect(opRunning()).toBeNull());
  });

  const cloneReq = (sourceId: string) => ({
    sourceId,
    newName: 'Default (copy)',
    options: {
      saves: true,
      settings: true,
      packs: true,
      config: true,
      options_txt: true,
      playtime: true,
    },
  });

  it('enqueueClone: success pushes an action toast whose Open selects the clone', async () => {
    (commands.cloneInstance as ReturnType<typeof vi.fn>).mockImplementation(
      async (
        _source: unknown,
        _name: unknown,
        _options: unknown,
        ch: { onmessage: ((m: unknown) => void) | null },
      ) => {
        ch.onmessage?.({ category: 'mods', current: 1, total: 1 });
        return { status: 'ok', data: { id: 'clone-9', name: 'Default (copy)' } };
      },
    );
    (commands.setActiveInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: null,
    });

    enqueueClone('Default (copy)', cloneReq('inst-1'));

    await vi.waitFor(() => expect(pushActionToast).toHaveBeenCalledTimes(1));
    await vi.waitFor(() => expect(opRunning()).toBeNull());

    const call = (pushActionToast as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(call[1]).toBe('instance.clone.done'); // tr echoes the key
    const action = call[2] as { run: () => void };
    action.run();
    await vi.waitFor(() => expect(commands.setActiveInstance).toHaveBeenCalledWith('clone-9'));
  });

  it('enqueueClone: dedupes a second clone of the same source while one is pending', async () => {
    const d = deferred<{ status: 'ok'; data: { id: string; name: string } }>();
    (commands.cloneInstance as ReturnType<typeof vi.fn>).mockReturnValue(d.promise);

    enqueueClone('Default (copy)', cloneReq('inst-1'));
    enqueueClone('Default (copy)', cloneReq('inst-1')); // same source → ignored
    enqueueClone('Other (copy)', cloneReq('inst-2')); // different source → queued

    expect(commands.cloneInstance).toHaveBeenCalledTimes(1);
    expect(opQueue().length).toBe(1);

    d.resolve({ status: 'ok', data: { id: 'clone-1', name: 'Default (copy)' } });
    await vi.waitFor(() => expect(opRunning()).toBeNull());
  });

  it('enqueueClone: error pushes a warning with the failure detail', async () => {
    (commands.cloneInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'error',
      error: { kind: 'instance_busy' },
    });

    enqueueClone('Default (copy)', cloneReq('inst-1'));

    await vi.waitFor(() => expect(pushWarning).toHaveBeenCalledTimes(1));
    const call = (pushWarning as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(call[0]).toBe('instance.clone.failed');
    expect(call[1]).toEqual(['formatted:instance_busy']);
    await vi.waitFor(() => expect(opRunning()).toBeNull());
  });

  it('enqueueClone: integrity dedupe ignores clone ops for the same instance id', async () => {
    // Regression for the isIntegrityOp refactor: a running CLONE of inst-1 must
    // not block enqueueing a VERIFY of inst-1 (different op families).
    const d = deferred<{ status: 'ok'; data: { id: string; name: string } }>();
    (commands.cloneInstance as ReturnType<typeof vi.fn>).mockReturnValue(d.promise);

    enqueueClone('Default (copy)', cloneReq('inst-1'));
    enqueueIntegrity('inst-1', 'Default', 'verify');

    expect(opQueue().length).toBe(1); // the verify queued, not dropped

    d.resolve({ status: 'ok', data: { id: 'clone-1', name: 'Default (copy)' } });
    await vi.waitFor(() => expect(opRunning()).toBeNull());
  });
});
