import { beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({ getDataLocation: vi.fn(), listen: vi.fn() }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { getDataLocation: h.getDataLocation },
  events: { dataMigrationProgress: { listen: h.listen } },
}));

import { dataLocation } from '$lib/settings/data-location.svelte';

// Its own file on purpose: `dataLocation` is a module singleton and these cases need one that has
// NEVER read a status. The tests run in order and build on each other.
const IO_ERROR = { status: 'error', error: { kind: 'io', path: 'x', details: 'ipc down' } };
const FINAL = {
  kind: 'restart_required',
  old_root: 'C:\Old',
  new_root: 'D:\New',
  leftovers: [],
  old_root_intact: false,
  old_root_is_default: false,
  retry_possible: false,
};
const ok = (relocation: unknown) => ({
  status: 'ok',
  data: {
    effective: 'C:\Old',
    configured: null,
    fell_back: false,
    default_dir: 'C:\Default',
    relocation,
  },
});

beforeEach(() => {
  vi.clearAllMocks();
});

describe('dataLocation — the first status read', () => {
  it('a REJECTED read (transport failure) is reported, not thrown, and is not "loaded"', async () => {
    h.getDataLocation.mockRejectedValue(new Error('ipc channel closed'));
    await expect(dataLocation.init()).resolves.toBeUndefined();
    expect(dataLocation.error).toContain('ipc channel closed');
    expect(dataLocation.loading).toBe(false);
    expect(dataLocation.loaded).toBe(false);
  });

  it('a failed read does not latch: the next init() asks again', async () => {
    h.getDataLocation.mockResolvedValue(IO_ERROR);
    await dataLocation.init();
    expect(dataLocation.loaded).toBe(false);
    await dataLocation.init();
    expect(h.getDataLocation).toHaveBeenCalledTimes(2);
  });

  it('concurrent init() calls share one read, and a late success still surfaces the move', async () => {
    h.getDataLocation.mockResolvedValue(ok(FINAL));
    await Promise.all([dataLocation.init(), dataLocation.init()]);
    expect(h.getDataLocation).toHaveBeenCalledTimes(1);
    expect(dataLocation.loaded).toBe(true);
    expect(dataLocation.error).toBeNull();
    expect(dataLocation.relocation.kind).toBe('restart_required');
  });

  it('once a read has succeeded, init() is a no-op', async () => {
    await dataLocation.init();
    expect(h.getDataLocation).not.toHaveBeenCalled();
  });
});
