import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstalledMod, InstanceWithStatus, MissingModStatus } from '$lib/ipc/bindings';

const {
  modsListInstalled,
  scanInstanceModCompat,
  checkInstanceModCompat,
  getPlaytime,
  modpackStatus,
  modsLastUpdateCheck,
} = vi.hoisted(() => ({
  modsListInstalled: vi.fn(),
  scanInstanceModCompat: vi.fn(),
  checkInstanceModCompat: vi.fn(),
  getPlaytime: vi.fn(),
  modpackStatus: vi.fn(),
  modsLastUpdateCheck: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled,
    scanInstanceModCompat,
    checkInstanceModCompat,
    getPlaytime,
    modpackStatus,
    modsLastUpdateCheck,
  },
}));

import { createInstanceStats } from '$lib/instances/instance-stats.svelte';
import { ensureCompatScan, invalidateCompatScan } from '$lib/mods/compat-scan.svelte';
import { __resetLiveVerdictsForTests } from '$lib/mods/installed/compat-check.svelte';
import { __resetUpdateCheckStoreForTests } from '$lib/mods/update-check-store.svelte';

// Minimal typed stubs — the composable only touches the named fields.
const mod = (enabled: boolean): InstalledMod => ({ enabled }) as unknown as InstalledMod;
let compatSeq = 0;
const compat = (loader_mismatch: boolean, live_checkable: boolean) =>
  ({ sha1: `jar-${++compatSeq}`, loader_mismatch, live_checkable }) as never;
const missing = (state: MissingModStatus['state']): MissingModStatus =>
  ({ state }) as unknown as MissingModStatus;
const forgeInstance = (id: string): InstanceWithStatus =>
  ({ id, mc_version: '1.20.1', loader: 'forge' }) as unknown as InstanceWithStatus;
const vanillaInstance = (id: string): InstanceWithStatus =>
  ({ id, mc_version: '1.20.1', loader: 'vanilla' }) as unknown as InstanceWithStatus;

describe('createInstanceStats', () => {
  beforeEach(() => {
    modsListInstalled.mockReset();
    scanInstanceModCompat.mockReset();
    getPlaytime.mockReset();
    modpackStatus.mockReset();
    modsLastUpdateCheck.mockReset();
    // The compat scan is an app-wide singleton shared with the Installed tab —
    // without this, a test reusing an earlier test's (instance, mc, loader) key
    // is deduplicated away and asserts against the previous test's entries.
    invalidateCompatScan();
    __resetLiveVerdictsForTests();
    // The persisted update check is held once per profile for the whole app, too.
    __resetUpdateCheckStoreForTests();
    // The fire-and-forget live ensure needs an answer; deciding nothing is
    // the neutral default. Cases that care override it.
    checkInstanceModCompat.mockResolvedValue({ status: 'ok', data: [] });
  });

  describe('refreshInstalledStats', () => {
    it('splits installed mods into total / enabled / disabled', async () => {
      modsListInstalled.mockResolvedValue({
        status: 'ok',
        data: [mod(true), mod(true), mod(false)],
      });
      const s = createInstanceStats();
      await s.refreshInstalledStats('i1');
      expect(modsListInstalled).toHaveBeenCalledWith('i1');
      expect(s.installedStats).toEqual({ total: 3, enabled: 2, disabled: 1 });
    });

    it('resets to zeros on a null instance id without calling the backend', async () => {
      const s = createInstanceStats();
      await s.refreshInstalledStats(null);
      expect(modsListInstalled).not.toHaveBeenCalled();
      expect(s.installedStats).toEqual({ total: 0, enabled: 0, disabled: 0 });
    });

    it('resets to zeros when the backend errors, rather than keeping the previous instance', async () => {
      modsListInstalled.mockResolvedValueOnce({ status: 'ok', data: [mod(true), mod(true)] });
      const s = createInstanceStats();
      await s.refreshInstalledStats('i1');
      expect(s.installedStats).toEqual({ total: 2, enabled: 2, disabled: 0 });

      // The labels are per-instance, so a retained value silently attributes the
      // previous instance's mods to this one.
      modsListInstalled.mockResolvedValueOnce({ status: 'error', error: { kind: 'x' } });
      await s.refreshInstalledStats('i2');
      expect(s.installedStats).toEqual({ total: 0, enabled: 0, disabled: 0 });
    });
  });

  describe('refreshIncompatible', () => {
    it('counts every family mismatch, live-checkable or not (spec D5)', async () => {
      // The family verdict is offline-authoritative: `scan_instance` already
      // excludes multi-loader jars and Connector setups, so every flagged
      // entry is a foreign FILE the loader will silently skip. The old
      // `!live_checkable` exclusion kept a Forge→Fabric switch invisible on
      // the Overview.
      scanInstanceModCompat.mockResolvedValue({
        status: 'ok',
        data: [compat(true, false), compat(true, false), compat(true, true), compat(false, false)],
      });
      const s = createInstanceStats();
      await s.refreshIncompatible('i1', [forgeInstance('i1')]);
      expect(scanInstanceModCompat).toHaveBeenCalledWith('i1');
      expect(s.compatHints.size).toBe(3);
    });

    it('scans a vanilla instance too, and gets 0 by computation', async () => {
      // The old short-circuit returned 0 without scanning, which left the
      // PREVIOUS instance's entries in the shared store — the Installed tab then
      // read them and the two surfaces disagreed. A vanilla instance has no
      // loader family, so `compat_verdict` yields no mismatch for any jar and
      // the honest answer falls out of the scan.
      scanInstanceModCompat.mockResolvedValue({ status: 'ok', data: [compat(false, false)] });
      const s = createInstanceStats();
      await s.refreshIncompatible('i1', [vanillaInstance('i1')]);
      expect(scanInstanceModCompat).toHaveBeenCalledWith('i1');
      expect(s.compatHints.size).toBe(0);
    });

    it('clears the shared scan when the requested id is not in the instances list', async () => {
      const s = createInstanceStats();
      await s.refreshIncompatible('ghost', [forgeInstance('i1')]);
      expect(scanInstanceModCompat).not.toHaveBeenCalled();
      expect(s.compatHints.size).toBe(0);
    });

    it('follows the shared store without a refresh of its own', async () => {
      // The count used to be a `$state` copied out of the store, so a scan run
      // by the Installed tab (or its manual re-check) moved
      // the chip and left the Overview showing the old number. Reading through
      // makes that drift unrepresentable.
      scanInstanceModCompat.mockResolvedValue({ status: 'ok', data: [compat(true, false)] });
      const s = createInstanceStats();
      expect(s.compatHints.size).toBe(0);

      await ensureCompatScan('i1', '1.20.1', 'forge');
      expect(s.compatHints.size).toBe(1);
    });

    it('forces a rescan when asked, so a mod change is not deduplicated away', async () => {
      // (instance, mc, loader) is unchanged by installing a mod, so without
      // `force` the store legitimately skips the call and no surface ever
      // notices the new jar.
      scanInstanceModCompat.mockResolvedValue({ status: 'ok', data: [] });
      const s = createInstanceStats();
      await s.refreshIncompatible('i1', [forgeInstance('i1')]);
      await s.refreshIncompatible('i1', [forgeInstance('i1')]);
      expect(scanInstanceModCompat).toHaveBeenCalledTimes(1);

      await s.refreshIncompatible('i1', [forgeInstance('i1')], { force: true });
      expect(scanInstanceModCompat).toHaveBeenCalledTimes(2);
    });

    it('hands the Overview each flagged mod with its reason, for the profile it scanned', async () => {
      // The Overview decides each mod's level with the rows' own statusOf, so it needs the
      // reason — the live half is looked up under the triple this refresh ran for.
      scanInstanceModCompat.mockResolvedValue({
        status: 'ok',
        data: [
          { sha1: 'x', loader_mismatch: true, detected_loader: 'Fabric', live_checkable: false },
          { sha1: 'y', loader_mismatch: false, live_checkable: true },
        ],
      });
      checkInstanceModCompat.mockResolvedValue({
        status: 'ok',
        data: [{ sha1: 'y', name: 'y', status: { status: 'incompatible' } }],
      });
      const s = createInstanceStats();
      await s.refreshIncompatible('i1', [forgeInstance('i1')]);
      await vi.waitFor(() => expect(s.compatHints.size).toBe(2));
      expect([...s.compatHints]).toEqual([
        ['x', { key: 'loader', detected: 'Fabric' }],
        ['y', { key: 'noRelease' }],
      ]);
    });
  });

  describe('refreshPlaytime', () => {
    it('stores the returned stats', async () => {
      getPlaytime.mockResolvedValue({
        status: 'ok',
        data: {
          total_seconds: 120,
          session_count: 3,
          last_session_seconds: 40,
          last_session_unix_ms: 1700000000000,
        },
      });
      const s = createInstanceStats();
      await s.refreshPlaytime('i1');
      expect(s.playtime.total_seconds).toBe(120);
      expect(s.playtime.session_count).toBe(3);
    });

    it('resets to the never-played zero stats on null id', async () => {
      const s = createInstanceStats();
      await s.refreshPlaytime(null);
      expect(getPlaytime).not.toHaveBeenCalled();
      expect(s.playtime).toEqual({
        total_seconds: 0,
        session_count: 0,
        last_session_seconds: 0,
        last_session_unix_ms: null,
      });
    });
  });

  describe('refreshPackStatus / unresolvedMissing', () => {
    it('derives the unresolved subset (missing + different_version)', async () => {
      modpackStatus.mockResolvedValue({
        status: 'ok',
        data: {
          missing_mods: [
            missing('missing'),
            missing('installed'),
            missing('substituted'),
            missing('different_version'),
          ],
        },
      });
      const s = createInstanceStats();
      await s.refreshPackStatus('i1');
      expect(s.packMissingMods).toHaveLength(4);
      expect(s.unresolvedMissing.map((m) => m.state)).toEqual(['missing', 'different_version']);
    });

    it('is empty for a non-pack instance (null status data)', async () => {
      modpackStatus.mockResolvedValue({ status: 'ok', data: null });
      const s = createInstanceStats();
      await s.refreshPackStatus('i1');
      expect(s.packMissingMods).toEqual([]);
      expect(s.unresolvedMissing).toEqual([]);
    });
  });

  describe('refreshUpdateCount', () => {
    const upd = (kind: string) => ({ state: { kind } }) as never;

    it('counts the pending updates of the persisted check', async () => {
      modsLastUpdateCheck.mockResolvedValue({
        status: 'ok',
        data: {
          checked_at_secs: 1,
          results: [upd('update_available'), upd('up_to_date'), upd('update_available')],
        },
      });
      const s = createInstanceStats();
      await s.refreshUpdateCount('i1');
      expect(modsLastUpdateCheck).toHaveBeenCalledWith('i1');
      expect(s.updateCount).toBe(2);
    });

    it('says "not known", never 0, when nothing was checked or the file could not be read', async () => {
      const s = createInstanceStats();
      modsLastUpdateCheck.mockResolvedValue({ status: 'ok', data: null });
      await s.refreshUpdateCount('i1');
      expect(s.updateCount).toBeNull();
      modsLastUpdateCheck.mockResolvedValue({ status: 'error', error: { kind: 'io' } });
      await s.refreshUpdateCount('i1');
      expect(s.updateCount).toBeNull();
    });

    it('a slower answer for the previous profile never lands on the next one', async () => {
      let answer: (v: unknown) => void = () => {};
      modsLastUpdateCheck
        .mockReturnValueOnce(
          new Promise((r) => {
            answer = r;
          }),
        )
        .mockResolvedValueOnce({ status: 'ok', data: null });
      const s = createInstanceStats();
      const first = s.refreshUpdateCount('old');
      await s.refreshUpdateCount('new');
      answer({ status: 'ok', data: { checked_at_secs: 1, results: [upd('update_available')] } });
      await first;
      expect(s.updateCount).toBeNull();
    });
  });
});
