/**
 * Two surfaces, one scan. The Overview must stay network-free (offline-decidable
 * mismatches only); the Installed tab folds its live platform verdicts on top.
 * Sharing the scan must not collapse them into the same number.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  scanInstanceModCompat: vi.fn(),
  checkInstanceModCompat: vi.fn(),
  modsVersions: vi.fn(),
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

const rows = (...shas: string[]) =>
  shas.map((sha1) => ({ installed: { sha1, source: 'modrinth' as const, project_id: 'p' } }));

function check(rs: ReturnType<typeof rows>) {
  return createCompatCheck(
    () => 'i1',
    () => '1.21.1',
    () => 'neoforge',
    () => rs,
  );
}

describe('the Overview and the Installed tab over one shared scan', () => {
  beforeEach(() => {
    mocks.scanInstanceModCompat.mockReset();
    mocks.checkInstanceModCompat.mockReset();
    mocks.modsVersions.mockReset();
    invalidateCompatScan();
    __resetLiveVerdictsForTests();
  });

  it('keeps the two views distinct: offline-only for the Overview, plus live for Installed', async () => {
    // `manual` is decidable offline. `suspect` needs the platform, which only
    // the Installed tab asks — and the platform has no build for this loader.
    mocks.scanInstanceModCompat.mockResolvedValue({
      status: 'ok',
      data: [
        entry('manual', true, false),
        entry('suspect', true, true),
        entry('fine', false, false),
      ],
    });
    mocks.modsVersions.mockResolvedValue({ status: 'ok', data: [] });

    const rs = rows('manual', 'suspect', 'fine');
    const compat = check(rs);
    await compat.runOfflineScan();

    expect(offlineMismatchCount()).toBe(1); // Overview — no network verdict folded in
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
    const compat = check(rows('a', 'b'));
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
    const compat = check(rows('a'));
    await compat.runOfflineScan();
    expect(compat.incompatibleCount).toBe(0);

    // The jar is replaced on disk with a Fabric-only build.
    mocks.scanInstanceModCompat.mockResolvedValueOnce({
      status: 'ok',
      data: [entry('a', true, false)],
    });
    mocks.checkInstanceModCompat.mockResolvedValue({ status: 'ok', data: [] });
    await compat.runLiveCheck();

    expect(compat.incompatibleCount).toBe(1);
    expect(compat.hintFor('a')).toEqual({ key: 'loader', detected: 'Fabric' });
  });
});
