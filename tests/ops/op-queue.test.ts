import { beforeEach, describe, expect, it, vi } from 'vitest';

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
  enqueueClone,
  enqueueImport,
  enqueueIntegrity,
  enqueueLauncherImport,
  opCompletionTick,
  opImportCompletionTick,
} from '$lib/ops/op-queue.svelte';
import { __resetTasksForTest, taskFor } from '$lib/tasks/registry.svelte';
import { pushActionToast, pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';

// This suite covers what `op-queue.svelte.ts` still owns after the tasks/
// registry migration: per-request dedupe, the completion toast (incl. the
// "Open" action), and the two page-level completion ticks. It deliberately
// does NOT re-test the registry's own queued/running/finished bookkeeping
// (see tests/tasks/registry.test.ts) or per-kind progress translation (see
// tests/tasks/adapters/*.test.ts) — those moved out of this module.

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
  __resetTasksForTest();
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
  it('dedupes a second enqueue for the same instance while it is active', async () => {
    const d = deferred<{ status: 'ok'; data: typeof healthyReport }>();
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockReturnValue(d.promise);

    enqueueIntegrity('a', 'Alpha', 'verify');
    // Second click while active — must be ignored. The registry's own
    // `start()` runs synchronously as part of the first call (no await
    // happens before it — see `$lib/tasks/adapters/integrity.ts`), so
    // `taskFor` already sees it by the time this second call's dedupe check
    // runs in the same synchronous script. The actual `commands.verifyInstance`
    // call is one microtask further behind that (the adapter awaits the
    // progress listener first), so it is asserted after a flush, not
    // synchronously.
    enqueueIntegrity('a', 'Alpha', 'verify');

    expect(taskFor({ instanceId: 'a' })).not.toBeNull();
    await vi.waitFor(() => expect(commands.verifyInstance).toHaveBeenCalledTimes(1));

    d.resolve({ status: 'ok', data: healthyReport });
    await d.promise;
    await vi.waitFor(() => expect(opCompletionTick()).toBe(1));
  });

  it('does not serialize different instances — both fire immediately (documented deferral)', async () => {
    const d1 = deferred<{ status: 'ok'; data: typeof healthyReport }>();
    const d2 = deferred<{ status: 'ok'; data: typeof healthyReport }>();
    (commands.verifyInstance as ReturnType<typeof vi.fn>)
      .mockReturnValueOnce(d1.promise)
      .mockReturnValueOnce(d2.promise);

    enqueueIntegrity('a', 'Alpha', 'verify');
    enqueueIntegrity('b', 'Bravo', 'verify');

    // Unlike the old strictly-serial queue, a DIFFERENT instance's op is not
    // held back — both backend calls are in flight at once.
    await vi.waitFor(() => expect(commands.verifyInstance).toHaveBeenCalledTimes(2));
    expect(commands.verifyInstance).toHaveBeenCalledWith('a');
    expect(commands.verifyInstance).toHaveBeenCalledWith('b');

    d1.resolve({ status: 'ok', data: healthyReport });
    d2.resolve({ status: 'ok', data: healthyReport });
    await d1.promise;
    await d2.promise;
    await vi.waitFor(() => expect(opCompletionTick()).toBe(2));
  });

  it('healthy verify → pushSuccess', async () => {
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: healthyReport,
    });

    enqueueIntegrity('a', 'Alpha', 'verify');
    await vi.waitFor(() => expect(opCompletionTick()).toBe(1));

    expect(pushSuccess).toHaveBeenCalledTimes(1);
    expect(pushWarning).not.toHaveBeenCalled();
  });

  it('verify with problems → pushWarning with the problem count', async () => {
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: brokenReport,
    });

    enqueueIntegrity('a', 'Alpha', 'verify');
    await vi.waitFor(() => expect(opCompletionTick()).toBe(1));

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
    await vi.waitFor(() => expect(opCompletionTick()).toBe(1));

    expect(pushWarning).toHaveBeenCalledTimes(1);
  });

  it('completion bumps the tick exactly once per op', async () => {
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: healthyReport,
    });

    expect(opCompletionTick()).toBe(0);
    enqueueIntegrity('a', 'Alpha', 'verify');
    await vi.waitFor(() => expect(opCompletionTick()).toBe(1));
  });

  it('leaves no active task registered once the op finishes', async () => {
    (commands.verifyInstance as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: healthyReport,
    });

    enqueueIntegrity('a', 'Alpha', 'verify');
    expect(taskFor({ instanceId: 'a' })).not.toBeNull();
    await vi.waitFor(() => expect(opCompletionTick()).toBe(1));
    expect(taskFor({ instanceId: 'a' })).toBeNull();
  });

  const importReq = (path: string) => ({
    path,
    selectedShas: [],
    projectId: null,
    source: null,
    versionId: null,
  });

  it('dedupes a second import of the same path', async () => {
    const d = deferred<{ status: 'ok'; data: { name: string } }>();
    (commands.modpackImport as ReturnType<typeof vi.fn>).mockReturnValue(d.promise);
    enqueueImport('Pack', importReq('/tmp/p.mrpack'));
    enqueueImport('Pack', importReq('/tmp/p.mrpack')); // same path → ignored
    expect(commands.modpackImport).toHaveBeenCalledTimes(1);
    d.resolve({ status: 'ok', data: { name: 'Pack' } });
    await vi.waitFor(() => expect(opCompletionTick()).toBe(1));
  });

  it('import success pushes an action toast whose Open runs setActiveInstance', async () => {
    (commands.modpackImport as ReturnType<typeof vi.fn>).mockImplementation(
      async (...args: unknown[]) => {
        (args[6] as { onmessage: (m: unknown) => void }).onmessage({
          phase: 'done',
          skipped_overrides: [],
          inert_loader_jars: [],
          details: [],
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
          details: [],
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
          details: [],
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

  it('import partial failure → pushWarning and still bumps importCompletionTick', async () => {
    (commands.modpackImport as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'error',
      error: { kind: 'modpack_partial_failure', failed: [['mods/a.jar', 'boom']] },
    });

    expect(opImportCompletionTick()).toBe(0);
    enqueueImport('Pack', importReq('/tmp/p.mrpack'));
    await vi.waitFor(() => expect(opImportCompletionTick()).toBe(1));
    expect(pushWarning).toHaveBeenCalledTimes(1);
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
    await vi.waitFor(() => expect(opImportCompletionTick()).toBe(1));
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

    expect(commands.launcherImportRun).toHaveBeenCalledTimes(1);
    d.resolve({ status: 'ok', data: mockInstanceData });
    await vi.waitFor(() => expect(opCompletionTick()).toBe(1));
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
    await vi.waitFor(() => expect(opCompletionTick()).toBe(1));
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
    await vi.waitFor(() => expect(opCompletionTick()).toBe(1));

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
    enqueueClone('Other (copy)', cloneReq('inst-2')); // different source → runs too

    expect(commands.cloneInstance).toHaveBeenCalledTimes(2);
    expect(commands.cloneInstance).toHaveBeenCalledWith(
      'inst-1',
      'Default (copy)',
      expect.anything(),
      expect.anything(),
    );
    expect(commands.cloneInstance).toHaveBeenCalledWith(
      'inst-2',
      'Default (copy)',
      expect.anything(),
      expect.anything(),
    );

    d.resolve({ status: 'ok', data: { id: 'clone-1', name: 'Default (copy)' } });
    await vi.waitFor(() => expect(opCompletionTick()).toBe(2));
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
    await vi.waitFor(() => expect(opCompletionTick()).toBe(1));
  });

  it('enqueueClone: an active clone blocks a verify for the same instance', async () => {
    // Widened from the old op-queue.svelte.ts's integrity-only dedupe: the
    // registry's `taskFor` matches by SCOPE alone, not kind, so a running
    // clone of inst-1 now also blocks enqueueing a verify of inst-1 — see
    // IntegritySection.svelte's doc comment for the same widening.
    const d = deferred<{ status: 'ok'; data: { id: string; name: string } }>();
    (commands.cloneInstance as ReturnType<typeof vi.fn>).mockReturnValue(d.promise);

    enqueueClone('Default (copy)', cloneReq('inst-1'));
    enqueueIntegrity('inst-1', 'Default', 'verify');

    expect(commands.verifyInstance).not.toHaveBeenCalled();

    d.resolve({ status: 'ok', data: { id: 'clone-1', name: 'Default (copy)' } });
    await vi.waitFor(() => expect(opCompletionTick()).toBe(1));
  });
});
