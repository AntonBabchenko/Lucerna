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

import { commands } from '$lib/ipc/bindings';
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

  it('marks the task failed on an error result (install)', async () => {
    vi.mocked(commands.modsInstallWithDeps).mockResolvedValue({
      status: 'error',
      error: { kind: 'instance_not_found', id: 'i' },
    } as never);

    const outcome = await installModWithDeps('i', 'X', primary, []);
    expect(taskList()[0].state).toBe('failed');
    expect(outcome.status).toBe('error');
  });

  it('marks the task failed on an error result (update)', async () => {
    vi.mocked(commands.modsUpdateOne).mockResolvedValue({
      status: 'error',
      error: { kind: 'instance_not_found', id: 'i' },
    } as never);

    const outcome = await updateMod('i', 'X', 'sha1', target);
    expect(taskList()[0].state).toBe('failed');
    expect(outcome.status).toBe('error');
  });

  it('marks the task failed when the command throws (bridge failure)', async () => {
    vi.mocked(commands.modsInstallWithDeps).mockRejectedValue(new Error('bridge died'));
    await installModWithDeps('i', 'X', primary, []);
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
