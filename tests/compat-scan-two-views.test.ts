/**
 * Two surfaces, one scan — and since spec D4, one COUNT. The offline scan
 * stays the network-free primitive (`offlineMismatchCount` — the Manage
 * summary and the offline fallback read it); the Installed chip and the
 * Overview row both show the union of offline mismatches and the keyed live
 * verdicts, so they can no longer disagree (locked C6).
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  scanInstanceModCompat: vi.fn(),
  checkInstanceModCompat: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));

import type { ModLocalCompat } from '$lib/ipc/bindings';
import {
  ensureCompatScan,
  invalidateCompatScan,
  offlineMismatchCount,
} from '$lib/mods/compat-scan.svelte';
import {
  __resetLiveVerdictsForTests,
  createCompatCheck,
  ensureLiveCompat,
  knownIncompatibleCount,
} from '$lib/mods/installed/compat-check.svelte';

const entry = (sha1: string, mismatch: boolean, liveCheckable: boolean): ModLocalCompat => ({
  sha1,
  loader_mismatch: mismatch,
  detected_loader: mismatch ? 'Fabric' : null,
  live_checkable: liveCheckable,
  platform_mismatch: false,
  platform_axis: null,
  platform_declared: null,
});

function check() {
  return createCompatCheck(
    () => 'i1',
    () => '1.21.1',
    () => 'neoforge',
  );
}

describe('the Overview and the Installed tab over one shared scan', () => {
  beforeEach(() => {
    mocks.scanInstanceModCompat.mockReset();
    mocks.checkInstanceModCompat.mockReset();
    mocks.checkInstanceModCompat.mockResolvedValue({ status: 'ok', data: [] });
    invalidateCompatScan();
    __resetLiveVerdictsForTests();
  });

  it('family mismatches are offline-authoritative; the live check cannot shrink the count', async () => {
    // Both foreign-family jars flag offline (spec D5) — even though the live
    // check says `suspect`'s project publishes builds for this platform, the
    // FILE on disk is still foreign and the loader will skip it.
    mocks.scanInstanceModCompat.mockResolvedValue({
      status: 'ok',
      data: [
        entry('manual', true, false),
        entry('suspect', true, true),
        entry('fine', false, false),
      ],
    });
    mocks.checkInstanceModCompat.mockResolvedValue({
      status: 'ok',
      data: [{ sha1: 'suspect', name: 'suspect', status: { status: 'compatible' } }],
    });

    const compat = check();
    await compat.runOfflineScan();

    expect(offlineMismatchCount()).toBe(2); // the network-free primitive counts both
    expect([...compat.incompatibleShas].sort()).toEqual(['manual', 'suspect']);
  });

  it('shows the Installed tab the mismatch the Overview already knows about', async () => {
    // The exact divergence observed on a real instance: the banner counted two
    // loader-mismatched jars while the chip showed none, because each surface
    // held its own copy of the scan.
    mocks.scanInstanceModCompat.mockResolvedValue({
      status: 'ok',
      data: [entry('a', true, false), entry('b', true, false)],
    });
    // The Overview scans first…
    await ensureCompatScan('i1', '1.21.1', 'neoforge');
    expect(offlineMismatchCount()).toBe(2);

    // …and the Installed tab mounting later sees the same two without rescanning.
    const compat = check();
    expect(compat.incompatibleCount).toBe(2);
    expect(mocks.scanInstanceModCompat).toHaveBeenCalledTimes(1);
  });

  it('re-checks loader mismatches on the manual button, not just platform verdicts', async () => {
    // `runLiveCheck` used to write only the live map, so a loader-family
    // mismatch could never be surfaced by the button labelled
    // "Check compatibility" — it answered "nothing found".
    mocks.scanInstanceModCompat.mockResolvedValueOnce({
      status: 'ok',
      data: [entry('a', false, false)],
    });
    const compat = check();
    await compat.runOfflineScan();
    expect(compat.incompatibleCount).toBe(0);

    // The jar is replaced on disk with a Fabric-only build.
    mocks.scanInstanceModCompat.mockResolvedValueOnce({
      status: 'ok',
      data: [entry('a', true, false)],
    });
    await compat.runLiveCheck();

    expect(compat.incompatibleCount).toBe(1);
    expect(compat.hintFor('a')).toEqual({ key: 'loader', detected: 'Fabric' });
  });

  it('the Overview union matches the chip once live verdicts land (spec D4)', async () => {
    // A live-only incompatible: permissive file range (no offline mismatch),
    // but the project publishes no build for this platform. Before D4 the
    // Overview stayed silent about it forever (observed live 2026-08-11: chip
    // said 4, Обзор said nothing).
    mocks.scanInstanceModCompat.mockResolvedValue({
      status: 'ok',
      data: [entry('live-only', false, true)],
    });
    mocks.checkInstanceModCompat.mockResolvedValue({
      status: 'ok',
      data: [{ sha1: 'live-only', name: 'live-only', status: { status: 'incompatible' } }],
    });

    await ensureCompatScan('i1', '1.21.1', 'neoforge');
    // Pure read before the ensure: falls back to the offline count.
    expect(knownIncompatibleCount('i1', '1.21.1', 'neoforge')).toBe(0);

    await ensureLiveCompat('i1', '1.21.1', 'neoforge');
    expect(offlineMismatchCount()).toBe(0); // the primitive stays offline-only
    expect(knownIncompatibleCount('i1', '1.21.1', 'neoforge')).toBe(1); // the Overview row
    const compat = check();
    expect(compat.incompatibleCount).toBe(1); // …and the chip agrees
  });
});
