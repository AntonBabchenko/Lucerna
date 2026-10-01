/**
 * The persisted update check, held once per profile for the whole app (spec 2026-09-28 §5.5, plan
 * A18): the Installed tab's badges and «проверено …» and the Overview's «Обновлений: N» read the
 * same copy, so a check run on the Installed tab reaches the Overview at once. A stored read never
 * overwrites something newer — a later read, rows dropped here, or this session's own check whose
 * save failed (the file then still holds an older check, or none).
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  modsLastUpdateCheck: vi.fn(),
  modsCheckUpdates: vi.fn(),
  modsListHolds: vi.fn(),
  modsGetCurseforgeKeyStatus: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));

import { createInstanceStats } from '$lib/instances/instance-stats.svelte';
import { createUpdateCheck } from '$lib/mods/installed/update-check.svelte';
import {
  __resetUpdateCheckStoreForTests,
  dropUpdateRows,
  loadStoredUpdateCheck,
  pendingUpdateCount,
  storedUpdateCheck,
} from '$lib/mods/update-check-store.svelte';

const row = (sha1: string, kind: 'update_available' | 'up_to_date' = 'update_available') =>
  ({
    sha1,
    name: sha1.toUpperCase(),
    source: 'modrinth',
    project_id: `p${sha1}`,
    current_version_id: 'v1',
    current_version_number: '1.0',
    state:
      kind === 'update_available'
        ? { kind, target: { version_id: 'v2', version_number: '2.0', name: 'n' } }
        : { kind },
  }) as never;
const stored = (secs: number, results: unknown[]) => ({
  status: 'ok',
  data: { checked_at_secs: secs, results },
});
const nowSecs = () => Math.floor(Date.now() / 1000);
const deferred = () => {
  let land: (v: unknown) => void = () => {};
  const promise = new Promise((r) => {
    land = r;
  });
  return { promise, land };
};

beforeEach(() => {
  vi.clearAllMocks();
  __resetUpdateCheckStoreForTests();
  mocks.modsLastUpdateCheck.mockResolvedValue({ status: 'ok', data: null });
  mocks.modsListHolds.mockResolvedValue({ status: 'ok', data: [] });
  mocks.modsGetCurseforgeKeyStatus.mockResolvedValue({ status: 'ok', data: 'set' });
});

describe('the persisted update check (one copy per profile)', () => {
  it('a check run on the Installed tab reaches the Overview count at once (A18)', async () => {
    const stats = createInstanceStats();
    await stats.refreshUpdateCount('i');
    expect(stats.updateCount).toBeNull();

    mocks.modsCheckUpdates.mockResolvedValue({
      status: 'ok',
      data: [row('a'), row('b', 'up_to_date')],
    });
    const updates = createUpdateCheck(
      () => 'i',
      async () => {},
    );
    await updates.checkUpdates();

    // No refreshUpdateCount in between: the Overview reads the same copy the check wrote.
    expect(stats.updateCount).toBe(1);
    updates.dispose();
  });

  it('of two reads for a profile, the one started last decides', async () => {
    const slow = deferred();
    mocks.modsLastUpdateCheck
      .mockReturnValueOnce(slow.promise)
      .mockResolvedValueOnce(stored(100, [row('a')]));
    const first = loadStoredUpdateCheck('i');
    await loadStoredUpdateCheck('i');
    slow.land(stored(50, [row('a'), row('gone')]));
    await first;
    expect(storedUpdateCheck('i')?.results.map((r) => r.sha1)).toEqual(['a']);
    expect(storedUpdateCheck('i')?.checkedAtMs).toBe(100_000);
  });

  it('a read that left before rows were dropped here never brings them back', async () => {
    mocks.modsLastUpdateCheck.mockResolvedValueOnce(stored(100, [row('a'), row('b')]));
    await loadStoredUpdateCheck('i');
    const slow = deferred();
    mocks.modsLastUpdateCheck.mockReturnValueOnce(slow.promise);
    const reading = loadStoredUpdateCheck('i');
    // An update just replaced a's jar: the read in flight was answered before that.
    dropUpdateRows('i', (c) => c.sha1 === 'a');
    slow.land(stored(100, [row('a'), row('b')]));
    await reading;
    expect(storedUpdateCheck('i')?.results.map((r) => r.sha1)).toEqual(['b']);
    expect(pendingUpdateCount('i')).toBe(1);
  });

  it('keeps this session’s check when the saved file is older or missing (its save failed)', async () => {
    mocks.modsCheckUpdates.mockResolvedValue({ status: 'ok', data: [row('a')] });
    const updates = createUpdateCheck(
      () => 'i',
      async () => {},
    );
    await updates.checkUpdates();

    // The file still holds the check before this one…
    mocks.modsLastUpdateCheck.mockResolvedValueOnce(stored(1, [row('old')]));
    await loadStoredUpdateCheck('i');
    expect(storedUpdateCheck('i')?.results.map((r) => r.sha1)).toEqual(['a']);
    // …or none at all.
    mocks.modsLastUpdateCheck.mockResolvedValueOnce({ status: 'ok', data: null });
    await loadStoredUpdateCheck('i');
    expect(storedUpdateCheck('i')?.results.map((r) => r.sha1)).toEqual(['a']);

    // Once the file holds this check, its rows are the backend's again (gone jars filtered).
    mocks.modsLastUpdateCheck.mockResolvedValueOnce(stored(nowSecs(), []));
    await loadStoredUpdateCheck('i');
    expect(storedUpdateCheck('i')?.results).toEqual([]);
    expect(pendingUpdateCount('i')).toBe(0);
    updates.dispose();
  });

  it('a stored check that cannot be read is "not checked" — no stale count', async () => {
    mocks.modsLastUpdateCheck.mockResolvedValueOnce(stored(100, [row('a')]));
    await loadStoredUpdateCheck('i');
    expect(pendingUpdateCount('i')).toBe(1);

    mocks.modsLastUpdateCheck.mockResolvedValueOnce({ status: 'error', error: { kind: 'io' } });
    await loadStoredUpdateCheck('i');
    expect(pendingUpdateCount('i')).toBeNull();

    mocks.modsLastUpdateCheck.mockResolvedValueOnce(stored(100, [row('a')]));
    await loadStoredUpdateCheck('i');
    mocks.modsLastUpdateCheck.mockRejectedValueOnce(new Error('bridge down'));
    await loadStoredUpdateCheck('i');
    expect(storedUpdateCheck('i')).toBeNull();
  });

  it('one check per profile at a time: a second click joins the running one', async () => {
    const slow = deferred();
    mocks.modsCheckUpdates.mockReturnValueOnce(slow.promise);
    const updates = createUpdateCheck(
      () => 'i',
      async () => {},
    );
    const a = updates.checkUpdates();
    const b = updates.checkUpdates();
    expect(updates.checking).toBe(true);
    slow.land({ status: 'ok', data: [row('a')] });
    await Promise.all([a, b]);
    expect(mocks.modsCheckUpdates).toHaveBeenCalledTimes(1);
    expect(updates.checking).toBe(false);
    expect(updates.updateCount).toBe(1);
    updates.dispose();
  });
});
