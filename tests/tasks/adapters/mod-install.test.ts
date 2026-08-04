import { beforeEach, describe, expect, it, vi } from 'vitest';

// Capture the listener callback at module-eval time via vi.hoisted so the
// vi.mock factory (which is hoisted itself) can access it. This adapter now
// subscribes PER CALL (mirrors game-install.test.ts's shape), not at module
// load, so there is only ever one live listener at a time in these tests.
const listeners = vi.hoisted(() => ({
  modInstallProgress: null as ((event: { payload: unknown }) => void) | null,
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsInstallWithDeps: vi.fn(),
    modsUpdateOne: vi.fn(),
  },
  events: {
    modInstallProgress: {
      listen: vi.fn((cb: (event: { payload: unknown }) => void) => {
        listeners.modInstallProgress = cb;
        return Promise.resolve(() => {
          listeners.modInstallProgress = null;
        });
      }),
    },
  },
}));

import { commands, events } from '$lib/ipc/bindings';
import { installModWithDeps, updateMod } from '$lib/tasks/adapters/mod-install';
import { __resetTasksForTest, taskList } from '$lib/tasks/registry.svelte';

function emit(payload: unknown) {
  listeners.modInstallProgress?.({ payload });
}

const primary = { source: 'modrinth', project_id: 'p', version_id: 'v' } as const;
const target = {
  source: 'modrinth',
  project_id: 'p',
  version_id: 'v2',
  name: 'X',
  version_number: '2.0',
  mc_versions: ['1.21'],
  loaders: ['fabric'],
  primary_file: { filename: 'x.jar', sha1: 'abc', size: 1 },
  deps: [],
  published_at: null,
} as never;

describe('mod-install adapter', () => {
  beforeEach(() => {
    __resetTasksForTest();
    vi.clearAllMocks();
    listeners.modInstallProgress = null;
  });

  it('tags a mod install as mod-install, scoped to the instance, concurrent lane', async () => {
    vi.mocked(commands.modsInstallWithDeps).mockResolvedValue({
      status: 'ok',
      data: { primary_name: 'X', installed_dependencies: [], details: [] },
    } as never);

    await installModWithDeps('i', 'X', primary, []);

    const task = taskList()[0];
    expect(task.kind).toBe('mod-install');
    expect(task.lane).toBe('concurrent');
    expect(task.scope).toEqual({ instanceId: 'i' });
    expect(task.state).toBe('ok');
  });

  it('carries InstallSummary.details onto the finished task (the per-mod report)', async () => {
    const detail = {
      name: 'X',
      install_path: 'mods/x.jar',
      origin: 'modrinth',
      host: null,
      bytes: 100,
      sha1: 'abc',
      outcome: 'installed',
    };
    vi.mocked(commands.modsInstallWithDeps).mockResolvedValue({
      status: 'ok',
      data: { primary_name: 'X', installed_dependencies: [], details: [detail] },
    } as never);

    await installModWithDeps('i', 'X', primary, []);
    expect(taskList()[0].details).toEqual([detail]);
  });

  it('tags a mod UPDATE as mod-update, not mod-install — the whole point of this adapter', async () => {
    vi.mocked(commands.modsUpdateOne).mockResolvedValue({ status: 'ok', data: null } as never);

    await updateMod('i', 'X', 'sha1', target);

    const task = taskList()[0];
    expect(task.kind).toBe('mod-update');
    expect(task.lane).toBe('concurrent');
    expect(task.scope).toEqual({ instanceId: 'i' });
    expect(task.state).toBe('ok');
  });

  it('feeds current/total from modInstallProgress ticks onto the task', async () => {
    vi.mocked(commands.modsInstallWithDeps).mockImplementation(async () => {
      emit({
        phase: 'downloading',
        instance_id: 'i',
        project_id: 'p',
        current: 2,
        total: 5,
      });
      return { status: 'ok', data: { primary_name: 'X', installed_dependencies: [], details: [] } };
    });

    await installModWithDeps('i', 'X', primary, []);
    expect(taskList()[0].progress).toEqual({ current: 2, total: 5, unit: 'files' });
  });

  it('treats total = 0 as indeterminate', async () => {
    vi.mocked(commands.modsInstallWithDeps).mockImplementation(async () => {
      emit({ phase: 'downloading', instance_id: 'i', project_id: 'p', current: 0, total: 0 });
      return { status: 'ok', data: { primary_name: 'X', installed_dependencies: [], details: [] } };
    });

    await installModWithDeps('i', 'X', primary, []);
    expect(taskList()[0].progress).toBeNull();
  });

  it('ignores a progress tick for a different instance', async () => {
    vi.mocked(commands.modsInstallWithDeps).mockImplementation(async () => {
      emit({ phase: 'downloading', instance_id: 'OTHER', project_id: 'p', current: 9, total: 9 });
      return { status: 'ok', data: { primary_name: 'X', installed_dependencies: [], details: [] } };
    });

    await installModWithDeps('i', 'X', primary, []);
    expect(taskList()[0].progress).toBeNull();
  });

  // Pins the drop-in-replacement contract: the wrapper must resolve with
  // EXACTLY the `Result` shape `commands.modsInstallWithDeps` /
  // `commands.modsUpdateOne` themselves return — the raw `IpcError`, not a
  // pre-formatted `{message}` — so every call site's existing
  // `res.status === 'error'` / `formatError(res.error)` handling keeps
  // compiling and behaving identically after the one-line swap.
  it('resolves with the same Result shape as commands.modsInstallWithDeps (ok)', async () => {
    const data = { primary_name: 'X', installed_dependencies: [], details: [] };
    vi.mocked(commands.modsInstallWithDeps).mockResolvedValue({ status: 'ok', data } as never);

    const result = await installModWithDeps('i', 'X', primary, []);
    expect(result).toEqual({ status: 'ok', data });
  });

  it('resolves with the same Result shape as commands.modsInstallWithDeps (error, raw IpcError untouched)', async () => {
    const error = { kind: 'instance_not_found', id: 'i' } as const;
    vi.mocked(commands.modsInstallWithDeps).mockResolvedValue({ status: 'error', error } as never);

    const result = await installModWithDeps('i', 'X', primary, []);
    expect(result).toEqual({ status: 'error', error });
    expect(taskList()[0].state).toBe('failed');
  });

  it('resolves with the same Result shape as commands.modsUpdateOne (ok)', async () => {
    vi.mocked(commands.modsUpdateOne).mockResolvedValue({ status: 'ok', data: null } as never);

    const result = await updateMod('i', 'X', 'sha1', target);
    expect(result).toEqual({ status: 'ok', data: null });
  });

  it('resolves with the same Result shape as commands.modsUpdateOne (error, raw IpcError untouched)', async () => {
    const error = { kind: 'instance_not_found', id: 'i' } as const;
    vi.mocked(commands.modsUpdateOne).mockResolvedValue({ status: 'error', error } as never);

    const result = await updateMod('i', 'X', 'sha1', target);
    expect(result).toEqual({ status: 'error', error });
    expect(taskList()[0].state).toBe('failed');
  });

  // A bridge failure (a real thrown Error, not a typed IpcError) propagates
  // out of `commands.modsInstallWithDeps` / `commands.modsUpdateOne`
  // themselves per `typedError`'s doc comment at the bottom of bindings.ts —
  // the wrapper must not swallow it into a resolved `{status:'error'}`, or
  // its behavior would diverge from the command it replaces. The task still
  // lands in a terminal `failed` state first (via the catch's `finish()`) so
  // a thrown error never wedges the operations strip.
  it('marks the task failed AND rethrows when modsInstallWithDeps throws (matches the command)', async () => {
    vi.mocked(commands.modsInstallWithDeps).mockRejectedValue(new Error('bridge died'));

    await expect(installModWithDeps('i', 'X', primary, [])).rejects.toThrow('bridge died');
    expect(taskList()[0].state).toBe('failed');
  });

  it('marks the task failed AND rethrows when modsUpdateOne throws (matches the command)', async () => {
    vi.mocked(commands.modsUpdateOne).mockRejectedValue(new Error('bridge died'));

    await expect(updateMod('i', 'X', 'sha1', target)).rejects.toThrow('bridge died');
    expect(taskList()[0].state).toBe('failed');
  });

  it('unsubscribes the listener once the call settles', async () => {
    vi.mocked(commands.modsInstallWithDeps).mockResolvedValue({
      status: 'ok',
      data: { primary_name: 'X', installed_dependencies: [], details: [] },
    } as never);
    await installModWithDeps('i', 'X', primary, []);
    expect(listeners.modInstallProgress).toBeNull();
  });

  it('keeps a concurrent install and update on the same instance as two separate tasks', async () => {
    vi.mocked(commands.modsInstallWithDeps).mockResolvedValue({
      status: 'ok',
      data: { primary_name: 'X', installed_dependencies: [], details: [] },
    } as never);
    vi.mocked(commands.modsUpdateOne).mockResolvedValue({ status: 'ok', data: null } as never);

    await Promise.all([
      installModWithDeps('i', 'X', primary, []),
      updateMod('i', 'Y', 'sha1', target),
    ]);

    expect(taskList().length).toBe(2);
    const kinds = taskList()
      .map((t) => t.kind)
      .sort();
    expect(kinds).toEqual(['mod-install', 'mod-update']);
  });
});

// A failed progress subscription must not cost the user the install. This
// bit CI on #352: four unrelated suites mock `$lib/ipc/bindings` with only
// `commands`, so `events` was undefined, the subscribe threw, and the
// command was never invoked at all — 19 tests failed for a reason that had
// nothing to do with what they were testing. Progress is decoration; the
// install is the point. Mirrors `op-queue.svelte.ts`'s long-standing
// try/catch around its own listener.
describe('progress subscription is best-effort', () => {
  // This block sits outside the file's main describe, so the outer
  // beforeEach does not reach it — clear the mocks here too or a sibling
  // test's call count leaks in.
  beforeEach(() => {
    vi.clearAllMocks();
    __resetTasksForTest();
  });

  it('still installs when the event bridge is unavailable', async () => {
    const listen = events.modInstallProgress.listen as ReturnType<typeof vi.fn>;
    listen.mockRejectedValueOnce(new Error('no tauri runtime'));
    (commands.modsInstallWithDeps as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: { primary_name: 'X', installed_dependencies: [], details: [] },
    });

    const r = await installModWithDeps('i1', 'X', primary, []);

    expect(commands.modsInstallWithDeps).toHaveBeenCalledTimes(1);
    expect(r.status).toBe('ok');
    expect(taskList()[0].state).toBe('ok');
  });
});
