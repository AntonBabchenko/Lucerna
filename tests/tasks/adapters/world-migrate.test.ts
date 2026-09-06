import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: ((m: unknown) => void) | null = null;
  },
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    worldMigrate: vi.fn(),
  },
}));

import type { MigrationMode, MigrationOutcome, MigrationProgress } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { isPartialOutcome, migrateWorld } from '$lib/tasks/adapters/world-migrate';
import { __resetTasksForTest, cancelQueued, taskList } from '$lib/tasks/registry.svelte';
import type { TaskProgress } from '$lib/tasks/types';

// Same cast tests/clone-runner.test.ts uses: the factory above installs a
// bare `vi.fn()`, but the import keeps the real bindings type.
const migrate = commands.worldMigrate as ReturnType<typeof vi.fn>;

type ProgressChannel = { onmessage: ((m: MigrationProgress) => void) | null };

const outcome: MigrationOutcome = {
  final_folder_name: 'My World',
  path: 'renamed',
  datapacks: [],
  datapacks_folders_copied: 0,
  links_skipped: 0,
  source_state: { kind: 'untouched' },
  backups_moved: 0,
  backups_left: 0,
};

const req = (fromInstance: string, mode: MigrationMode = 'copy') => ({
  fromInstance,
  worldFolder: 'My World',
  toInstance: 'inst-b',
  mode,
});

function snapshot(): { phase: string | null; progress: TaskProgress | null } {
  const task = taskList()[0];
  return { phase: task.phase, progress: task.progress };
}

describe('world-migrate adapter', () => {
  beforeEach(() => {
    __resetTasksForTest();
    vi.clearAllMocks();
  });

  it('registers a serial task scoped to the source instance and returns the outcome', async () => {
    migrate.mockResolvedValue({ status: 'ok', data: outcome });

    const result = await migrateWorld('My World', req('inst-a'));

    expect(result).toEqual({ status: 'ok', outcome });
    const task = taskList()[0];
    expect(task.kind).toBe('world-migrate');
    expect(task.lane).toBe('serial');
    expect(task.title).toBe('My World');
    expect(task.scope).toEqual({ instanceId: 'inst-a' });
    expect(task.state).toBe('ok');

    expect(migrate).toHaveBeenCalledOnce();
    const args = migrate.mock.calls[0];
    expect(args.slice(0, 4)).toEqual(['inst-a', 'My World', 'inst-b', 'copy']);
    expect(args[4]).toBeDefined(); // the progress Channel
  });

  it('maps every phase: bytes for copying, files for linking and backups, no bar otherwise', async () => {
    const seen: Array<ReturnType<typeof snapshot>> = [];
    migrate.mockImplementation(async (_from, _world, _to, _mode, ch: ProgressChannel) => {
      ch.onmessage?.({ phase: 'moving', current: 0, total: 0 });
      seen.push(snapshot());
      ch.onmessage?.({ phase: 'copying', current: 512, total: 2048 });
      seen.push(snapshot());
      ch.onmessage?.({ phase: 'linking', current: 2, total: 5 });
      seen.push(snapshot());
      ch.onmessage?.({ phase: 'backups', current: 1, total: 4 });
      seen.push(snapshot());
      ch.onmessage?.({ phase: 'finalising', current: 0, total: 0 });
      seen.push(snapshot());
      return { status: 'ok', data: outcome };
    });

    await migrateWorld('My World', req('inst-a', 'move'));

    expect(seen).toEqual([
      { phase: 'moving', progress: null },
      { phase: 'copying', progress: { current: 512, total: 2048, unit: 'bytes' } },
      { phase: 'linking', progress: { current: 2, total: 5, unit: 'files' } },
      { phase: 'backups', progress: { current: 1, total: 4, unit: 'files' } },
      { phase: 'finalising', progress: null },
    ]);
  });

  it('draws no bar when a counter arrives null — specta exports both sides as number | null', async () => {
    const seen: Array<ReturnType<typeof snapshot>> = [];
    migrate.mockImplementation(async (_from, _world, _to, _mode, ch: ProgressChannel) => {
      ch.onmessage?.({ phase: 'copying', current: null, total: null });
      seen.push(snapshot());
      ch.onmessage?.({ phase: 'linking', current: 3, total: null });
      seen.push(snapshot());
      return { status: 'ok', data: outcome };
    });

    await migrateWorld('My World', req('inst-a'));

    expect(seen).toEqual([
      { phase: 'copying', progress: null },
      { phase: 'linking', progress: null },
    ]);
  });

  it('publishes a byte rate for copying and clears it on the next file-counted phase', async () => {
    const hasRate: boolean[] = [];
    migrate.mockImplementation(async (_from, _world, _to, _mode, ch: ProgressChannel) => {
      ch.onmessage?.({ phase: 'copying', current: 1024, total: 4096 });
      hasRate.push(taskList()[0].rate !== null);
      ch.onmessage?.({ phase: 'linking', current: 1, total: 2 });
      hasRate.push(taskList()[0].rate !== null);
      return { status: 'ok', data: outcome };
    });

    await migrateWorld('My World', req('inst-a'));

    expect(hasRate).toEqual([true, false]);
  });

  it('marks the task failed and hands back the raw IPC error', async () => {
    migrate.mockResolvedValue({ status: 'error', error: { kind: 'instance_busy' } });

    const result = await migrateWorld('My World', req('inst-a'));

    expect(result).toEqual({ status: 'error', error: { kind: 'instance_busy' } });
    expect(taskList()[0].state).toBe('failed');
  });

  it('marks the task failed when the bridge throws', async () => {
    migrate.mockRejectedValue(new Error('bridge died'));

    const result = await migrateWorld('My World', req('inst-a'));

    expect(result).toEqual({ status: 'error', error: expect.any(Error) });
    expect(taskList()[0].state).toBe('failed');
  });

  it('is not cancellable once running; cancelling it while queued never fires the command', async () => {
    let resolveFirst: (v: unknown) => void = () => {};
    const firstCall = new Promise((resolve) => {
      resolveFirst = resolve;
    });
    migrate.mockReturnValueOnce(firstCall);

    const first = migrateWorld('First', req('inst-a'));
    const second = migrateWorld('Second', req('inst-c'));
    // Flush the microtasks without settling `firstCall` — same idiom as
    // tests/tasks/adapters/serial-gate.test.ts.
    await Promise.resolve();
    await Promise.resolve();

    const running = taskList().find((t) => t.title === 'First');
    expect(running?.state).toBe('running');
    expect(running?.caps).toEqual({ cancellable: false, reorderable: false });

    const queued = taskList().find((t) => t.title === 'Second');
    expect(queued?.state).toBe('queued');
    cancelQueued(queued?.id ?? '');

    resolveFirst({ status: 'ok', data: outcome });
    await first;
    const secondOutcome = await second;

    expect(migrate).toHaveBeenCalledTimes(1);
    expect(secondOutcome).toEqual({ status: 'cancelled' });
    expect(taskList().find((t) => t.title === 'Second')).toBeUndefined();
  });
});

describe('isPartialOutcome', () => {
  it('is false for an outcome with nothing left to read', () => {
    expect(isPartialOutcome(outcome)).toBe(false);
  });

  it('is true for a datapack left as a copy, skipped links, backups left, or a source not removed', () => {
    expect(
      isPartialOutcome({
        ...outcome,
        datapacks: [
          {
            filename: 'terralith.zip',
            result: { kind: 'left_as_copy', reason: { kind: 'name_held_by_different_pack' } },
          },
        ],
      }),
    ).toBe(true);
    expect(isPartialOutcome({ ...outcome, links_skipped: 1 })).toBe(true);
    expect(isPartialOutcome({ ...outcome, backups_left: 2 })).toBe(true);
    expect(
      isPartialOutcome({
        ...outcome,
        source_state: { kind: 'left_intact', reason: 'os error 32' },
      }),
    ).toBe(true);
  });
});
