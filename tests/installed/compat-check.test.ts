import { beforeEach, describe, expect, it, vi } from 'vitest';

const scanMock = vi.fn();
const liveMock = vi.fn();
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    scanInstanceModCompat: (...a: unknown[]) => scanMock(...a),
    checkInstanceModCompat: (...a: unknown[]) => liveMock(...a),
  },
}));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));

import { invalidateCompatScan } from '$lib/mods/compat-scan.svelte';
import {
  __resetLiveVerdictsForTests,
  createCompatCheck,
} from '$lib/mods/installed/compat-check.svelte';

function lc(
  sha: string,
  loader_mismatch: boolean,
  live_checkable: boolean,
  detected_loader: string | null = null,
) {
  return {
    sha1: sha,
    loader_mismatch,
    live_checkable,
    detected_loader,
    platform_mismatch: false,
    platform_axis: null,
    platform_declared: null,
  };
}

// A jar whose declared MC/loader range this instance does not provide — read
// off the file itself, independent of the loader-FAMILY axis and of
// `live_checkable`.
function platformMismatch(
  sha: string,
  axis: 'minecraft' | 'loader',
  declared: string,
  opts: { loader_mismatch?: boolean; live_checkable?: boolean } = {},
) {
  return {
    sha1: sha,
    loader_mismatch: opts.loader_mismatch ?? false,
    live_checkable: opts.live_checkable ?? true,
    detected_loader: null,
    platform_mismatch: true,
    platform_axis: axis,
    platform_declared: declared,
  };
}

const verdict = (sha1: string, status: 'compatible' | 'incompatible' | 'unknown') => ({
  sha1,
  name: sha1,
  status: { status },
});

function check() {
  return createCompatCheck(
    () => 'i',
    () => '1.21',
    () => 'forge',
  );
}

describe('createCompatCheck two-stage pipeline', () => {
  beforeEach(() => {
    scanMock.mockReset();
    liveMock.mockReset();
    // Safe default: the platform answers, decides nothing. Cases that care
    // override it.
    liveMock.mockResolvedValue({ status: 'ok', data: [] });
    // The offline scan is an app-wide store shared with the Overview, so it
    // outlives a single composable — reset it like any other module singleton,
    // and the keyed live-verdict store with it.
    invalidateCompatScan();
    __resetLiveVerdictsForTests();
  });

  it('manual suspect is flagged offline; nothing checkable → no live call at all', async () => {
    scanMock.mockResolvedValue({ status: 'ok', data: [lc('a', true, false, 'Fabric')] });
    const c = check();
    await c.runOfflineScan();
    expect([...c.incompatibleShas]).toEqual(['a']);
    expect(c.hintFor('a')).toEqual({ key: 'loader', detected: 'Fabric' });
    expect(liveMock).not.toHaveBeenCalled();
    c.dispose();
  });

  it('family suspect stays flagged even when the project publishes builds (spec D5)', async () => {
    // The Forge→Fabric silence: every project had fabric builds, so the live
    // «compatible» cleared the suspicion while FabricLoader skipped the ten
    // still-forge FILES on disk. Live enriches; it never clears the family axis
    // (multi-loader jars and Connector setups never flag in the scan at all).
    scanMock.mockResolvedValue({ status: 'ok', data: [lc('a', true, true, 'Forge')] });
    liveMock.mockResolvedValue({ status: 'ok', data: [verdict('a', 'compatible')] });
    const c = check();
    await c.runOfflineScan();
    expect([...c.incompatibleShas]).toEqual(['a']);
    expect(c.hintFor('a')).toEqual({ key: 'loader', detected: 'Forge' });
    expect(liveMock).toHaveBeenCalledWith('i', '1.21', 'forge');
    c.dispose();
  });

  it('family suspect with no builds either stays flagged with the loader hint', async () => {
    scanMock.mockResolvedValue({ status: 'ok', data: [lc('a', true, true, 'Forge')] });
    liveMock.mockResolvedValue({ status: 'ok', data: [verdict('a', 'incompatible')] });
    const c = check();
    await c.runOfflineScan();
    expect([...c.incompatibleShas]).toEqual(['a']);
    // The family hint names the FILE's problem; «нет сборки» would misdescribe it.
    expect(c.hintFor('a')).toEqual({ key: 'loader', detected: 'Forge' });
    c.dispose();
  });

  it('a live-only incompatible surfaces WITHOUT suspicion — the button reveals nothing new', async () => {
    // The maintainer's smoke finding (2026-08-12): a mod whose file range is
    // permissive (no loader suspicion) but whose project publishes no build
    // for this platform used to appear only after pressing the button. The
    // auto pass now runs the same full check, so it is flagged on tab open.
    scanMock.mockResolvedValue({ status: 'ok', data: [lc('a', false, true)] });
    liveMock.mockResolvedValue({ status: 'ok', data: [verdict('a', 'incompatible')] });
    const c = check();
    await c.runOfflineScan();
    expect([...c.incompatibleShas]).toEqual(['a']);
    expect(c.hintFor('a')).toEqual({ key: 'noRelease' });
    c.dispose();
  });

  it('a command-level failure adds no live verdicts and is retried on the next run', async () => {
    // A live-checkable mod with NO family suspicion: only the live check can
    // flag it, so a failed command must flag nothing — and stay undecided so
    // the next pipeline run asks again.
    scanMock.mockResolvedValue({ status: 'ok', data: [lc('a', false, true)] });
    liveMock.mockResolvedValueOnce({ status: 'error', error: 'HTTP 429' });
    const c = check();
    await c.runOfflineScan();
    expect(c.incompatibleCount).toBe(0);
    liveMock.mockResolvedValueOnce({ status: 'ok', data: [verdict('a', 'incompatible')] });
    await c.runOfflineScan();
    expect(c.incompatibleCount).toBe(1);
    expect(liveMock).toHaveBeenCalledTimes(2);
    c.dispose();
  });

  it('a decided triple is not re-queried on later runs', async () => {
    scanMock.mockResolvedValue({ status: 'ok', data: [lc('a', true, true, 'Forge')] });
    liveMock.mockResolvedValue({ status: 'ok', data: [verdict('a', 'compatible')] });
    const c = check();
    await c.runOfflineScan();
    await c.runOfflineScan();
    expect(liveMock).toHaveBeenCalledTimes(1);
    c.dispose();
  });

  it('concurrent pipeline runs share one in-flight live check', async () => {
    scanMock.mockResolvedValue({ status: 'ok', data: [lc('a', true, true, 'Fabric')] });
    let release: (v: unknown) => void = () => {};
    liveMock.mockImplementationOnce(
      () =>
        new Promise((res) => {
          release = res;
        }),
    );
    const c = check();
    const runA = c.runOfflineScan();
    // Let A get past the offline scan and park inside the live check.
    await new Promise((r) => setTimeout(r, 0));
    const runB = c.runOfflineScan(); // joins A's in-flight check, no second call
    release({ status: 'ok', data: [verdict('a', 'incompatible')] });
    await Promise.all([runA, runB]);
    expect(liveMock).toHaveBeenCalledTimes(1);
    expect([...c.incompatibleShas]).toEqual(['a']);
    c.dispose();
  });

  it('counts a platform mismatch even when the loader family is fine', async () => {
    // BiomesOPlenty on a downgraded instance: right family, wrong loader version.
    scanMock.mockResolvedValue({
      status: 'ok',
      data: [platformMismatch('bop', 'loader', '[61.0.2,)')],
    });
    const c = check();
    await c.runOfflineScan();
    expect(c.incompatibleCount).toBe(1);
    expect([...c.incompatibleShas]).toEqual(['bop']);
    c.dispose();
  });

  it('a live compatible verdict does not clear a platform mismatch', async () => {
    // The original defect: every one of the six projects publishes a build for
    // the target MC, so the live check answered "compatible" and the launcher
    // said nothing. The file on disk is still wrong.
    scanMock.mockResolvedValue({
      status: 'ok',
      data: [platformMismatch('bad-file', 'minecraft', '(1.21.10, 26.1.0)')],
    });
    liveMock.mockResolvedValue({
      status: 'ok',
      data: [verdict('bad-file', 'compatible')],
    });
    const c = check();
    await c.runOfflineScan();
    await c.runLiveCheck();
    expect([...c.incompatibleShas]).toContain('bad-file');
    c.dispose();
  });

  it('a platform mismatch is counted regardless of live_checkable', async () => {
    // The !live_checkable guard belongs to the loader-FAMILY axis only.
    scanMock.mockResolvedValue({
      status: 'ok',
      data: [
        platformMismatch('checkable', 'minecraft', '(1.21.10, 26.1.0)', {
          live_checkable: true,
        }),
        platformMismatch('not-checkable', 'minecraft', '(1.21.10, 26.1.0)', {
          live_checkable: false,
        }),
      ],
    });
    const c = check();
    await c.runOfflineScan();
    expect([...c.incompatibleShas].sort()).toEqual(['checkable', 'not-checkable']);
    c.dispose();
  });
});
