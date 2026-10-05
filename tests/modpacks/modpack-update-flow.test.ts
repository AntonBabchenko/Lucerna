import { beforeEach, describe, expect, it, vi } from 'vitest';

// Inline-factory mocks (same pattern as tests/ops/import-runner.test.ts);
// grab typed refs AFTER import to avoid any vi.mock hoisting pitfalls.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modpackFetchToTemp: vi.fn(),
    modpackComputeUpdate: vi.fn(),
    modpackApplyUpdate: vi.fn(),
    listWorldNames: vi.fn(),
  },
}));
vi.mock('$lib/ipc/format-error', () => ({
  formatError: vi.fn((e: { kind: string }) => `formatted:${e.kind}`),
}));
vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: ((m: unknown) => void) | null = null;
  },
}));

import type { InstanceWithStatus, ModpackVersionEntry } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { createModpackUpdateFlow } from '$lib/modpacks/modpack-update-flow.svelte';
import { __resetTasksForTest, taskList } from '$lib/tasks/registry.svelte';

const fetchToTemp = commands.modpackFetchToTemp as ReturnType<typeof vi.fn>;
const computeUpdate = commands.modpackComputeUpdate as ReturnType<typeof vi.fn>;
const applyUpdate = commands.modpackApplyUpdate as ReturnType<typeof vi.fn>;
const listWorldNames = commands.listWorldNames as ReturnType<typeof vi.fn>;

const inst = { id: 'i1', mrpack_source: 'modrinth', mrpack_project_id: 'p1' } as InstanceWithStatus;
const entry = { id: 'v2', version_number: '1.3.0' } as ModpackVersionEntry;
const flush = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  vi.clearAllMocks();
  fetchToTemp.mockResolvedValue({ status: 'ok', data: '/tmp/p.mrpack' });
  computeUpdate.mockResolvedValue({
    status: 'ok',
    data: { added: [], removed: [], updated: [], new_version_number: '1.3.0', version_bump: null },
  });
  applyUpdate.mockResolvedValue({
    status: 'ok',
    data: { instance: { id: 'i1' }, inert_loader_jars: [], details: [] },
  });
  listWorldNames.mockResolvedValue({ status: 'ok', data: [] });
});

describe('createModpackUpdateFlow', () => {
  it('prepare() fetches, computes the diff, and moves to confirming', async () => {
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    expect(fetchToTemp).toHaveBeenCalledWith('modrinth', 'p1', 'v2');
    expect(computeUpdate).toHaveBeenCalledWith('i1', '/tmp/p.mrpack');
    expect(flow.phase).toBe('confirming');
    expect(flow.diff).not.toBeNull();
  });

  it('prepare() surfaces a fetch error and returns to idle', async () => {
    fetchToTemp.mockResolvedValue({ status: 'error', error: { kind: 'io' } });
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    expect(flow.error).toBe('formatted:io');
    expect(flow.phase).toBe('idle');
    expect(flow.diff).toBeNull();
  });

  it('prepare() surfaces a compute-diff error and returns to idle', async () => {
    computeUpdate.mockResolvedValue({ status: 'error', error: { kind: 'io' } });
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    expect(fetchToTemp).toHaveBeenCalled();
    expect(flow.error).toBe('formatted:io');
    expect(flow.phase).toBe('idle');
    expect(flow.diff).toBeNull();
  });

  // `running` is work in flight. The confirm step waits on the user, so a
  // surface may be dismissed there; while the archive is fetched or the update
  // applied it must stay put. Answers are held until the test releases them,
  // and states are read first and asserted after: a failure never leaves a
  // held answer, or the serial task lane, blocking the tests below.
  it('is running while it fetches or applies, not while it waits for a confirm', async () => {
    let landFetch!: (v: unknown) => void;
    fetchToTemp.mockReturnValue(
      new Promise((r) => {
        landFetch = r;
      }),
    );
    let landApply!: (v: unknown) => void;
    applyUpdate.mockImplementation(
      () =>
        new Promise((r) => {
          landApply = r;
        }),
    );
    const flow = createModpackUpdateFlow();
    const seen: Array<[string, boolean]> = [];
    const look = () => seen.push([flow.phase, flow.running]);

    look();
    const prepared = flow.prepare(inst, entry);
    look();
    landFetch({ status: 'ok', data: '/tmp/p.mrpack' });
    await prepared;
    look();
    const applied = flow.confirm(inst);
    await flush();
    look();
    landApply({
      status: 'ok',
      data: { instance: { id: 'i1' }, inert_loader_jars: [], details: [] },
    });
    expect(await applied).toBe(true);
    look();

    expect(seen).toEqual([
      ['idle', false],
      ['preparing', true],
      ['confirming', false],
      ['applying', true],
      ['idle', false],
    ]);
  });

  // A cancel mid-prepare used to null the target id under a fetch that still
  // landed: the confirm step then held an archive with no version to record,
  // and confirming did nothing at all.
  it('cancel() leaves a prepare in flight alone, and its confirm still applies', async () => {
    let landFetch!: (v: unknown) => void;
    fetchToTemp.mockReturnValue(
      new Promise((r) => {
        landFetch = r;
      }),
    );
    const flow = createModpackUpdateFlow();
    const prepared = flow.prepare(inst, entry);
    flow.cancel();
    const afterCancel = flow.phase;
    landFetch({ status: 'ok', data: '/tmp/p.mrpack' });
    await prepared;
    expect(afterCancel).toBe('preparing');
    expect(await flow.confirm(inst)).toBe(true);
    // (instanceId, mrpackPath, newVersionId, …)
    expect(applyUpdate.mock.calls[0][1]).toBe('/tmp/p.mrpack');
    expect(applyUpdate.mock.calls[0][2]).toBe('v2');
  });

  // A cancel mid-apply used to show idle over an update that was still running.
  it('cancel() leaves an update being applied alone', async () => {
    let landApply!: (v: unknown) => void;
    applyUpdate.mockImplementation(
      () =>
        new Promise((r) => {
          landApply = r;
        }),
    );
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    const applied = flow.confirm(inst);
    await flush();
    flow.cancel();
    const afterCancel = { phase: flow.phase, running: flow.running };
    landApply({
      status: 'ok',
      data: { instance: { id: 'i1' }, inert_loader_jars: [], details: [] },
    });
    expect(await applied).toBe(true);
    expect(afterCancel).toEqual({ phase: 'applying', running: true });
    expect(flow.phase).toBe('idle');
  });

  // What a dismissed confirm relies on: the pending update is gone, so nothing
  // can apply it later.
  it('cancel() from the confirm step drops the pending update', async () => {
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    flow.cancel();
    expect(flow.phase).toBe('idle');
    expect(flow.diff).toBeNull();
    expect(await flow.confirm(inst)).toBe(false);
    expect(applyUpdate).not.toHaveBeenCalled();
  });

  // One update at a time. The confirm surfaces cover the page's own Update
  // buttons, and the flow does not count on that (a press on a dialog's scrim
  // used to drop the focus to the page): a second prepare under an open
  // confirm used to start another fetch, and the next confirm could then pair
  // one version's archive with the other's id.
  it('prepare() refuses while another update waits for its confirm', async () => {
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    const pending = flow.diff;
    await flow.prepare(inst, { id: 'v3', version_number: '1.4.0' } as ModpackVersionEntry);
    expect(fetchToTemp).toHaveBeenCalledTimes(1);
    expect(flow.phase).toBe('confirming');
    expect(flow.diff).toBe(pending);
    expect(await flow.confirm(inst)).toBe(true);
    expect(applyUpdate.mock.calls[0][2]).toBe('v2');
  });

  it('prepare() refuses while another is being fetched', async () => {
    let landFetch!: (v: unknown) => void;
    fetchToTemp.mockReturnValue(
      new Promise((r) => {
        landFetch = r;
      }),
    );
    const flow = createModpackUpdateFlow();
    const first = flow.prepare(inst, entry);
    const second = flow.prepare(inst, { id: 'v3', version_number: '1.4.0' } as ModpackVersionEntry);
    const fetches = fetchToTemp.mock.calls.length;
    landFetch({ status: 'ok', data: '/tmp/p.mrpack' });
    await Promise.all([first, second]);
    expect(fetches).toBe(1);
    expect(await flow.confirm(inst)).toBe(true);
    expect(applyUpdate.mock.calls[0][2]).toBe('v2');
  });

  // `typedError` rethrows a bridge failure instead of resolving to an error.
  // A prepare that let it escape stayed `preparing` (running) for good, and a
  // surface that stays open while running could never be closed.
  it('a bridge failure while preparing ends the prepare with its message', async () => {
    fetchToTemp.mockRejectedValue(new Error('bridge down'));
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    expect(flow.phase).toBe('idle');
    expect(flow.running).toBe(false);
    expect(flow.error).toBe('bridge down');
  });

  it('confirm() applies, maps installing_file into progress mid-flight, returns true', async () => {
    let release: (v: unknown) => void = () => {};
    let phaseCh: { onmessage: (m: unknown) => void } | null = null;
    applyUpdate.mockImplementation((...args: unknown[]) => {
      phaseCh = args[4] as { onmessage: (m: unknown) => void };
      return new Promise((r) => {
        release = r;
      });
    });
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    const p = flow.confirm(inst);
    await flush();
    expect(flow.phase).toBe('applying');
    phaseCh!.onmessage({ phase: 'installing_file', current: 3, total: 12, file_name: 'Sodium' });
    expect(flow.progress).toEqual({
      current: 3,
      total: 12,
      fileName: 'Sodium',
      phase: 'installing_file',
    });
    release({ status: 'ok', data: { instance: { id: 'i1' }, inert_loader_jars: [], details: [] } });
    expect(await p).toBe(true);
    expect(flow.phase).toBe('idle');
  });

  it('confirm() returns false and sets error on apply failure', async () => {
    applyUpdate.mockResolvedValue({ status: 'error', error: { kind: 'io' } });
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    const ok = await flow.confirm(inst);
    expect(ok).toBe(false);
    expect(flow.error).toBe('formatted:io');
    expect(flow.phase).toBe('idle');
  });

  // The apply step is a long-running job, so it belongs in the operations
  // strip like every other one. It was shipped in #347 with the adapter built
  // but never called — the strip stayed empty during a modpack update.
  it('registers a pack-update task in the registry while applying', async () => {
    __resetTasksForTest();
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);

    const p = flow.confirm(inst);
    await flush();

    const task = taskList().find((t) => t.kind === 'pack-update');
    expect(task).toBeDefined();
    expect(task?.scope.instanceId).toBe('i1');

    expect(await p).toBe(true);
    expect(taskList().find((t) => t.kind === 'pack-update')?.state).toBe('ok');
  });

  // The three consuming surfaces render their own inline progress off
  // `flow.progress`; registering a task must not take that away from them.
  it('keeps feeding the flow its own inline progress', async () => {
    __resetTasksForTest();
    let phaseCh: { onmessage: ((m: unknown) => void) | null } | undefined;
    let release!: (v: unknown) => void;
    applyUpdate.mockImplementation((...args: unknown[]) => {
      phaseCh = args[4] as typeof phaseCh;
      return new Promise((r) => {
        release = r;
      });
    });

    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    const p = flow.confirm(inst);
    await flush();

    phaseCh!.onmessage?.({ phase: 'installing_file', current: 5, total: 9, file_name: 'Iris' });
    expect(flow.progress).toEqual({
      current: 5,
      total: 9,
      fileName: 'Iris',
      phase: 'installing_file',
    });

    release({ status: 'ok', data: { instance: { id: 'i1' }, inert_loader_jars: [], details: [] } });
    await p;
  });

  it('prepare() counts the worlds for the backup choice', async () => {
    listWorldNames.mockResolvedValue({
      status: 'ok',
      data: [
        { folder_name: 'A', modified_unix_ms: 0 },
        { folder_name: 'B', modified_unix_ms: 0 },
      ],
    });
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    expect(listWorldNames).toHaveBeenCalledWith('i1');
    expect(flow.worldCount).toBe(2);
  });

  it('an unreadable world count is unknown, not zero', async () => {
    listWorldNames.mockResolvedValue({ status: 'error', error: { kind: 'io' } });
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    expect(flow.worldCount).toBeNull();
  });

  it('confirm() passes the backup choice and shows the world being backed up', async () => {
    let phaseCh: { onmessage: (m: unknown) => void } | null = null;
    let release: (v: unknown) => void = () => {};
    applyUpdate.mockImplementation((...args: unknown[]) => {
      phaseCh = args[4] as { onmessage: (m: unknown) => void };
      return new Promise((r) => {
        release = r;
      });
    });
    const flow = createModpackUpdateFlow();
    await flow.prepare(inst, entry);
    const p = flow.confirm(inst, { backupWorlds: true });
    await flush();
    expect(applyUpdate.mock.calls[0][3]).toBe(true);
    phaseCh!.onmessage({ phase: 'backing_up_world', current: 1, total: 2, world_name: 'Survival' });
    expect(flow.progress).toEqual({
      current: 1,
      total: 2,
      fileName: 'Survival',
      phase: 'backing_up_world',
    });
    // The backups are done and the files move in: the world line must not stay up.
    phaseCh!.onmessage({ phase: 'applying_changes' });
    expect(flow.progress).toEqual({ phase: 'applying_changes' });
    release({ status: 'ok', data: { instance: { id: 'i1' }, inert_loader_jars: [], details: [] } });
    expect(await p).toBe(true);
  });
});
