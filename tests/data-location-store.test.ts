import { beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({ getDataLocation: vi.fn(), listen: vi.fn(), unlisten: vi.fn() }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { getDataLocation: h.getDataLocation },
  events: { dataMigrationProgress: { listen: h.listen } },
}));

import { dataLocation } from '$lib/settings/data-location.svelte';

type Tick = { payload: { copied_bytes: number; total_bytes: number; phase: string } };
let emit: (e: Tick) => void = () => {};
const flush = () => new Promise((r) => setTimeout(r, 0));

const IDLE = { kind: 'idle' };
// The full generated `restart_required` shape: the backend decides `old_root_is_default` (canonical
// compare) and `retry_possible` (some leftover is not launcher-owned) — the frontend only reads them.
const FINAL = {
  kind: 'restart_required',
  old_root: 'C:\\Old',
  new_root: 'D:\\New',
  leftovers: ['logs'],
  old_root_intact: false,
  old_root_is_default: false,
  retry_possible: true,
};
const ok = (relocation: unknown) => ({
  status: 'ok',
  data: {
    effective: 'C:\\Old',
    configured: null,
    fell_back: false,
    default_dir: 'C:\\Default',
    relocation,
  },
});
const see = async (relocation: unknown) => {
  h.getDataLocation.mockResolvedValue(ok(relocation));
  await dataLocation.refresh();
};

// `dataLocation` is a module singleton: every test drives it to a known state through refresh()
// first. The `restartUnconfirmed` case is LAST on purpose — only a confirming read clears it.
beforeEach(() => {
  vi.clearAllMocks();
  h.listen.mockImplementation(async (cb: (e: Tick) => void) => {
    emit = cb;
    return h.unlisten;
  });
});

describe('dataLocation — relocation view', () => {
  it('reads a status without the relocation field as "no move", and exposes default_dir', async () => {
    h.getDataLocation.mockResolvedValue({
      status: 'ok',
      data: { effective: '/data', configured: null, fell_back: false },
    });
    await dataLocation.refresh();
    expect(dataLocation.relocation).toEqual({ kind: 'idle' });
    expect(dataLocation.defaultDir).toBeNull();
    await see(IDLE);
    expect(dataLocation.defaultDir).toBe('C:\\Default');
  });

  it('shows a running move from the status alone (the page was reloaded mid-move)', async () => {
    await see({ kind: 'running', phase: 'verifying' });
    expect(dataLocation.relocation).toEqual({
      kind: 'running',
      phase: 'verifying',
      progress: null,
    });
  });

  it('lets a progress tick override the phase, and releases the subscription on dispose', async () => {
    await see({ kind: 'running', phase: 'copying' });
    const dispose = dataLocation.attach();
    await flush();
    expect(h.listen).toHaveBeenCalledTimes(1);
    emit({ payload: { copied_bytes: 50, total_bytes: 200, phase: 'switching' } });
    expect(dataLocation.relocation).toMatchObject({
      kind: 'running',
      phase: 'switching',
      progress: { copied_bytes: 50 },
    });

    dispose();
    expect(h.unlisten).toHaveBeenCalledTimes(1);
    emit({ payload: { copied_bytes: 99, total_bytes: 200, phase: 'deleting' } });
    expect(dataLocation.relocation).toMatchObject({ phase: 'switching' });
  });

  it('releases a subscription whose owner went away before listen() resolved', async () => {
    let resolveListen: (un: () => void) => void = () => {};
    h.listen.mockReturnValue(
      new Promise((r) => {
        resolveListen = r;
      }),
    );
    dataLocation.attach()();
    resolveListen(h.unlisten);
    await flush();
    expect(h.unlisten).toHaveBeenCalledTimes(1);
  });

  it('flags a move this page did not start when it ends without switching', async () => {
    await see({ kind: 'running', phase: 'copying' });
    await see(IDLE);
    expect(dataLocation.orphanEnded).toBe(true);
    dataLocation.ackOrphanEnded();
    expect(dataLocation.orphanEnded).toBe(false);
  });

  it('shows "preparing" from the moment this page starts a move, and settles to idle on cancel', async () => {
    await see(IDLE);
    dataLocation.moveStarted();
    expect(dataLocation.relocation).toEqual({ kind: 'running', phase: null, progress: null });
    await dataLocation.moveSettled({ kind: 'cancelled' });
    expect(dataLocation.relocation).toEqual({ kind: 'idle' });
    // The page owned the command, so the panel reports the outcome — no toast.
    expect(dataLocation.orphanEnded).toBe(false);
  });

  it('takes the final state from the status and applies a retry result immutably', async () => {
    h.getDataLocation.mockResolvedValue(ok(FINAL));
    dataLocation.moveStarted();
    await dataLocation.moveSettled({ kind: 'restart_required' });
    expect(dataLocation.relocation).toEqual({ kind: 'restart_required', details: FINAL });

    const before = dataLocation.status;
    // What a successful "Try again" returns: nothing left, so nothing more to retry.
    dataLocation.applyRelocation({ ...FINAL, leftovers: [], retry_possible: false } as never);
    expect(dataLocation.status).not.toBe(before);
    expect(before?.relocation).toEqual(FINAL);
    expect(dataLocation.relocation).toMatchObject({
      details: { leftovers: [], retry_possible: false },
    });
  });

  it('keeps the final dialog when the status read cannot confirm restart_required', async () => {
    await see(IDLE);
    h.getDataLocation.mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: 'x', details: 'ipc down' },
    });
    dataLocation.moveStarted();
    await dataLocation.moveSettled({ kind: 'restart_required' });
    expect(dataLocation.relocation).toEqual({ kind: 'restart_required', details: null });
  });
});
