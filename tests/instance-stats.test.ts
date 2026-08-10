import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstalledMod, InstanceWithStatus, MissingModStatus } from '$lib/ipc/bindings';

const { modsListInstalled, scanInstanceModCompat, getPlaytime, modpackStatus } = vi.hoisted(() => ({
  modsListInstalled: vi.fn(),
  scanInstanceModCompat: vi.fn(),
  getPlaytime: vi.fn(),
  modpackStatus: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { modsListInstalled, scanInstanceModCompat, getPlaytime, modpackStatus },
}));

import { createInstanceStats } from '$lib/instances/instance-stats.svelte';
import { ensureCompatScan, invalidateCompatScan } from '$lib/mods/compat-scan.svelte';

// Minimal typed stubs — the composable only touches the named fields.
const mod = (enabled: boolean): InstalledMod => ({ enabled }) as unknown as InstalledMod;
const compat = (loader_mismatch: boolean, live_checkable: boolean) =>
  ({ loader_mismatch, live_checkable }) as never;
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
    // The compat scan is an app-wide singleton shared with the Installed tab —
    // without this, a test reusing an earlier test's (instance, mc, loader) key
    // is deduplicated away and asserts against the previous test's entries.
    invalidateCompatScan();
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
    it('counts only loader-mismatched, non-live-checkable jars', async () => {
      scanInstanceModCompat.mockResolvedValue({
        status: 'ok',
        // counts: mismatch+!checkable (yes), mismatch+checkable (no — platform
        // suspect, needs live confirm), no-mismatch (no).
        data: [compat(true, false), compat(true, false), compat(true, true), compat(false, false)],
      });
      const s = createInstanceStats();
      await s.refreshIncompatible('i1', [forgeInstance('i1')]);
      expect(scanInstanceModCompat).toHaveBeenCalledWith('i1');
      expect(s.incompatibleCount).toBe(2);
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
      expect(s.incompatibleCount).toBe(0);
    });

    it('clears the shared scan when the requested id is not in the instances list', async () => {
      const s = createInstanceStats();
      await s.refreshIncompatible('ghost', [forgeInstance('i1')]);
      expect(scanInstanceModCompat).not.toHaveBeenCalled();
      expect(s.incompatibleCount).toBe(0);
    });

    it('follows the shared store without a refresh of its own', async () => {
      // The count used to be a `$state` copied out of the store, so a scan run
      // by the Installed tab (or the manual "Check compatibility" button) moved
      // the chip and left the Overview showing the old number. Reading through
      // makes that drift unrepresentable.
      scanInstanceModCompat.mockResolvedValue({ status: 'ok', data: [compat(true, false)] });
      const s = createInstanceStats();
      expect(s.incompatibleCount).toBe(0);

      await ensureCompatScan('i1', '1.20.1', 'forge');
      expect(s.incompatibleCount).toBe(1);
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
});
