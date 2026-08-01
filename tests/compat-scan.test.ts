/**
 * The shared compatibility scan. Two surfaces read it — the Overview indicator
 * and the Installed tab's chip — and they used to each hold a private copy that
 * could drift. These cover the store's contract; `compat-scan-two-views` covers
 * the two surfaces staying distinct on top of it.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ scanInstanceModCompat: vi.fn(), modsVersions: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));

import type { ModLocalCompat } from '$lib/ipc/bindings';
import {
  compatScanEntries,
  ensureCompatScan,
  invalidateCompatScan,
  offlineMismatchCount,
} from '$lib/mods/compat-scan.svelte';

const entry = (sha1: string, mismatch: boolean, liveCheckable: boolean): ModLocalCompat => ({
  sha1,
  loader_mismatch: mismatch,
  detected_loader: mismatch ? 'Fabric' : null,
  live_checkable: liveCheckable,
});

const ok = (data: ModLocalCompat[]) => ({ status: 'ok' as const, data });

describe('compat-scan store', () => {
  beforeEach(() => {
    mocks.scanInstanceModCompat.mockReset();
    invalidateCompatScan();
  });

  it('scans once for a key and reuses the result', async () => {
    mocks.scanInstanceModCompat.mockResolvedValue(ok([entry('a', true, false)]));
    await ensureCompatScan('i1', '1.21.1', 'neoforge');
    await ensureCompatScan('i1', '1.21.1', 'neoforge');
    expect(mocks.scanInstanceModCompat).toHaveBeenCalledTimes(1);
    expect(compatScanEntries()).toHaveLength(1);
  });

  it('rescans when the instance, mc or loader changes', async () => {
    mocks.scanInstanceModCompat.mockResolvedValue(ok([]));
    await ensureCompatScan('i1', '1.21.1', 'neoforge');
    await ensureCompatScan('i2', '1.21.1', 'neoforge');
    await ensureCompatScan('i2', '1.20.1', 'neoforge');
    await ensureCompatScan('i2', '1.20.1', 'forge');
    expect(mocks.scanInstanceModCompat).toHaveBeenCalledTimes(4);
  });

  it('rescans on force even for the same key', async () => {
    mocks.scanInstanceModCompat.mockResolvedValue(ok([]));
    await ensureCompatScan('i1', '1.21.1', 'neoforge');
    await ensureCompatScan('i1', '1.21.1', 'neoforge', { force: true });
    expect(mocks.scanInstanceModCompat).toHaveBeenCalledTimes(2);
  });

  it('keeps the last good result when a scan fails', async () => {
    mocks.scanInstanceModCompat.mockResolvedValueOnce(ok([entry('a', true, false)]));
    await ensureCompatScan('i1', '1.21.1', 'neoforge');
    expect(offlineMismatchCount()).toBe(1);

    // An empty list reads as "nothing is wrong" — a transient error must never
    // be allowed to say that.
    mocks.scanInstanceModCompat.mockResolvedValueOnce({ status: 'error', error: 'offline' });
    await ensureCompatScan('i1', '1.21.1', 'neoforge', { force: true });
    expect(offlineMismatchCount()).toBe(1);
  });

  it('drops a superseded scan rather than writing the previous instance verdicts', async () => {
    const gate: { release: () => void } = { release: () => {} };
    mocks.scanInstanceModCompat.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          gate.release = () => resolve(ok([entry('stale', true, false)]));
        }),
    );
    mocks.scanInstanceModCompat.mockResolvedValueOnce(ok([entry('fresh', false, false)]));

    const slow = ensureCompatScan('i1', '1.21.1', 'neoforge');
    await ensureCompatScan('i2', '1.21.1', 'neoforge');
    gate.release();
    await slow;

    expect(compatScanEntries().map((e) => e.sha1)).toEqual(['fresh']);
    expect(offlineMismatchCount()).toBe(0);
  });

  it('counts only offline-decidable mismatches', async () => {
    mocks.scanInstanceModCompat.mockResolvedValue(
      ok([
        entry('manual-mismatch', true, false), // counted
        entry('platform-suspect', true, true), // needs a live check the Overview must not make
        entry('fine', false, false),
      ]),
    );
    await ensureCompatScan('i1', '1.21.1', 'neoforge');
    expect(offlineMismatchCount()).toBe(1);
  });

  it('clears when there is no instance', async () => {
    mocks.scanInstanceModCompat.mockResolvedValue(ok([entry('a', true, false)]));
    await ensureCompatScan('i1', '1.21.1', 'neoforge');
    await ensureCompatScan(null, null, null);
    expect(compatScanEntries()).toEqual([]);
    expect(offlineMismatchCount()).toBe(0);
  });
});
