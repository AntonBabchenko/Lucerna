/**
 * Live-verdict lifecycle (spec 2026-08-11-compat-verdict-truthfulness, D2).
 *
 * Observed live on a real instance (2026-08-11): navigating Обзор → back wiped
 * the manual check's verdicts (chip 4 → «всё в порядке»), while a version
 * change 1.21 → 1.20.1 KEPT them (chip flagged four perfect 1.20.1 builds with
 * verdicts computed against 1.21). Both are lifecycle bugs of the same store:
 * verdicts must survive component remounts and die with the platform triple
 * they were computed for.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  scanInstanceModCompat: vi.fn(),
  checkInstanceModCompat: vi.fn(),
  modsVersions: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));

import type { LoaderKind, ModLocalCompat } from '$lib/ipc/bindings';
import { invalidateCompatScan } from '$lib/mods/compat-scan.svelte';
import { createCompatCheck } from '$lib/mods/installed/compat-check.svelte';

const suspect = (sha1: string): ModLocalCompat => ({
  sha1,
  loader_mismatch: true,
  detected_loader: 'Fabric',
  live_checkable: true,
  platform_mismatch: false,
  platform_axis: null,
  platform_declared: null,
});

const rows = (...shas: string[]) =>
  shas.map((sha1) => ({ installed: { sha1, source: 'modrinth' as const, project_id: 'p' } }));

describe('live verdict lifecycle', () => {
  beforeEach(async () => {
    mocks.scanInstanceModCompat.mockReset();
    mocks.checkInstanceModCompat.mockReset();
    mocks.modsVersions.mockReset();
    invalidateCompatScan();
    // The live store is module-level by design (it must survive remounts);
    // tests must not inherit each other's verdicts.
    const mod = await import('$lib/mods/installed/compat-check.svelte');
    (mod as { __resetLiveVerdictsForTests?: () => void }).__resetLiveVerdictsForTests?.();
  });

  function makeCheck(getMc: () => string) {
    return createCompatCheck(
      () => 'i1',
      getMc,
      () => 'forge' as LoaderKind,
      () => rows('a'),
    );
  }

  it('verdicts survive re-creating the composable (tab remount)', async () => {
    mocks.scanInstanceModCompat.mockResolvedValue({ status: 'ok', data: [suspect('a')] });
    mocks.modsVersions.mockResolvedValue({ status: 'ok', data: [] }); // no build → incompatible

    const first = makeCheck(() => '1.21');
    await first.runOfflineScan();
    expect(first.incompatibleCount).toBe(1);

    // The user visits Обзор and comes back: InstalledModsView unmounts and a
    // fresh composable mounts. The verdict the manual/auto check already paid
    // for must still be there — no re-query, no «всё в порядке» flicker.
    const second = makeCheck(() => '1.21');
    expect(second.incompatibleCount).toBe(1);
    expect(second.hintFor('a')).toEqual({ key: 'noRelease' });
  });

  it('a verdict from the previous platform never counts, and the suspect is re-decided', async () => {
    mocks.scanInstanceModCompat.mockResolvedValue({ status: 'ok', data: [suspect('a')] });
    // Under 1.21 the project has no build (incompatible); under 1.20.1 it has
    // one (compatible). The sha never changes — exactly the live carryover.
    mocks.modsVersions.mockImplementation(async (_s: string, _p: string, mc: string) => ({
      status: 'ok',
      data: mc === '1.21' ? [] : [{ id: 'v-1201' }],
    }));

    let mc = '1.21';
    const compat = makeCheck(() => mc);
    await compat.runOfflineScan();
    expect(compat.incompatibleCount).toBe(1);

    // The user changes the instance to 1.20.1 and the pipeline re-runs.
    mc = '1.20.1';
    await compat.runOfflineScan();
    // The 1.21 verdict must not flag a mod on 1.20.1 — and the auto pipeline
    // must have ASKED again for the new platform rather than pinning the old
    // answer (the `!live.has(sha)` pin, audit #10).
    expect(compat.incompatibleCount).toBe(0);
    const askedFor = mocks.modsVersions.mock.calls.map((c) => c[2]);
    expect(askedFor).toContain('1.20.1');
  });

  it('returning to a platform restores its verdicts without a re-query', async () => {
    mocks.scanInstanceModCompat.mockResolvedValue({ status: 'ok', data: [suspect('a')] });
    mocks.modsVersions.mockImplementation(async (_s: string, _p: string, mc: string) => ({
      status: 'ok',
      data: mc === '1.21' ? [] : [{ id: 'v-1201' }],
    }));

    let mc = '1.21';
    const compat = makeCheck(() => mc);
    await compat.runOfflineScan(); // decides 'a' under 1.21 → incompatible
    mc = '1.20.1';
    await compat.runOfflineScan(); // decides 'a' under 1.20.1 → compatible
    const callsAfterBoth = mocks.modsVersions.mock.calls.length;

    mc = '1.21';
    await compat.runOfflineScan({ force: true }); // back — both verdicts already known
    expect(compat.incompatibleCount).toBe(1);
    expect(mocks.modsVersions.mock.calls.length).toBe(callsAfterBoth);
  });
});
