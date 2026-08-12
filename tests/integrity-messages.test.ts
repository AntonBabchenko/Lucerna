import { beforeEach, describe, expect, it } from 'vitest';
import { locale } from '$lib/i18n';
import { compatSummaryFromScan, loaderOutcomeToast } from '$lib/instances/integrity-messages';
import type { LoaderOutcome, ModLocalCompat } from '$lib/ipc/bindings';

// The functions under test call get(t) at invocation time. Pin to 'en'
// so assertions on English text hold regardless of the runner's OS locale.
beforeEach(() => {
  locale.set('en');
});

// ---------------------------------------------------------------------------
// loaderOutcomeToast
// ---------------------------------------------------------------------------

describe('loaderOutcomeToast', () => {
  it('returns null for unchanged', () => {
    const outcome: LoaderOutcome = { kind: 'unchanged' };
    expect(loaderOutcomeToast(outcome, '1.21')).toBeNull();
  });

  it('returns success toast for loader_updated containing version and mc', () => {
    const outcome: LoaderOutcome = {
      kind: 'loader_updated',
      loader: 'fabric',
      version: '0.16.5',
    };
    const result = loaderOutcomeToast(outcome, '1.21');
    expect(result).not.toBeNull();
    expect(result!.kind).toBe('success');
    expect(result!.text).toContain('0.16.5');
    expect(result!.text).toContain('1.21');
    expect(result!.text).toContain('Fabric');
  });

  it('returns success toast for loader_updated with NeoForge canonical name', () => {
    const outcome: LoaderOutcome = {
      kind: 'loader_updated',
      loader: 'neoforge',
      version: '21.1.0',
    };
    const result = loaderOutcomeToast(outcome, '1.21.1');
    expect(result).not.toBeNull();
    expect(result!.kind).toBe('success');
    expect(result!.text).toContain('NeoForge');
    expect(result!.text).toContain('21.1.0');
    expect(result!.text).toContain('1.21.1');
  });

  it('returns warning toast for loader_reset_to_vanilla', () => {
    const outcome: LoaderOutcome = {
      kind: 'loader_reset_to_vanilla',
      previous_loader: 'quilt',
    };
    const result = loaderOutcomeToast(outcome, '1.20.4');
    expect(result).not.toBeNull();
    expect(result!.kind).toBe('warning');
    expect(result!.text).toContain('Quilt');
    expect(result!.text).toContain('1.20.4');
    expect(result!.text.toLowerCase()).toContain('vanilla');
  });

  it('warning toast for loader_reset_to_vanilla includes Forge', () => {
    const outcome: LoaderOutcome = {
      kind: 'loader_reset_to_vanilla',
      previous_loader: 'forge',
    };
    const result = loaderOutcomeToast(outcome, '1.8.9');
    expect(result).not.toBeNull();
    expect(result!.kind).toBe('warning');
    expect(result!.text).toContain('Forge');
  });
});

// ---------------------------------------------------------------------------
// compatSummaryFromScan
// ---------------------------------------------------------------------------

function makeLocalCompat(sha1: string, overrides: Partial<ModLocalCompat> = {}): ModLocalCompat {
  return {
    sha1,
    loader_mismatch: false,
    detected_loader: null,
    live_checkable: true,
    platform_mismatch: false,
    platform_axis: null,
    platform_declared: null,
    ...overrides,
  };
}

describe('compatSummaryFromScan', () => {
  it('returns null for an empty scan', () => {
    expect(compatSummaryFromScan([], 0)).toBeNull();
  });

  it('returns null for a fully-clean scan', () => {
    const scan: ModLocalCompat[] = Array.from({ length: 15 }, (_, i) =>
      makeLocalCompat(`clean-${i}`),
    );
    expect(compatSummaryFromScan(scan, 15)).toBeNull();
  });

  it('reports the stale jars after a version change instead of zero', () => {
    // The reported defect: six 1.21.11 jars in a 1.20.1 instance. Every one of
    // those six PROJECTS publishes a 1.20.1 build, so the old platform-query
    // summary computed "0 of 15" and rendered nothing. The offline scan judges
    // the FILE, so it must report six.
    const stale = Array.from({ length: 6 }, (_, i) =>
      makeLocalCompat(`stale-${i}`, {
        platform_mismatch: true,
        loader_mismatch: false,
        live_checkable: true,
        platform_axis: 'loader',
        platform_declared: '[61.0.2,)',
      }),
    );
    const clean = Array.from({ length: 9 }, (_, i) => makeLocalCompat(`clean-${i}`));
    const scan = [...stale, ...clean];

    const result = compatSummaryFromScan(scan, 15);
    expect(result).not.toBeNull();
    expect(result).toContain('6 of 15');
  });

  it('counts an unconfirmed loader-family suspect the same way the Overview does', () => {
    const scan: ModLocalCompat[] = [
      makeLocalCompat('manual-suspect', { loader_mismatch: true, live_checkable: false }),
      makeLocalCompat('fine'),
    ];
    expect(compatSummaryFromScan(scan, 2)).toContain('1 of 2');
  });

  it('counts a live-checkable family mismatch too (spec D5)', () => {
    // Mirrors isOfflineMismatch's contract: the family verdict is
    // offline-authoritative (multi-loader jars and Connector setups never
    // flag inside the scan), so the Manage summary names a foreign-family
    // jar regardless of live-checkability - the silence after a loader
    // switch was exactly this exclusion.
    const scan: ModLocalCompat[] = [
      makeLocalCompat('suspect', { loader_mismatch: true, live_checkable: true }),
    ];
    expect(compatSummaryFromScan(scan, 1)).not.toBeNull();
  });

  it('vanilla with enabled mods warns as an instance-level condition (spec D9)', () => {
    // Per-jar verdicts correctly never flag on Vanilla (no family to
    // mismatch), yet Vanilla loads NO mods — the summary must say so itself.
    const scan: ModLocalCompat[] = [makeLocalCompat('a', {}), makeLocalCompat('b', {})];
    const text = compatSummaryFromScan(scan, 2, 'vanilla');
    expect(text).not.toBeNull();
    expect(text).toContain('2');
    expect(text).toMatch(/loader/i);
  });

  it('vanilla with no mods stays silent', () => {
    expect(compatSummaryFromScan([], 0, 'vanilla')).toBeNull();
  });

  it('a modded loader is unaffected by the loader parameter', () => {
    const scan: ModLocalCompat[] = [makeLocalCompat('a', {})];
    expect(compatSummaryFromScan(scan, 1, 'forge')).toBeNull();
  });

  it('mentions review in mods tab', () => {
    const scan: ModLocalCompat[] = [
      makeLocalCompat('a', { platform_mismatch: true, platform_axis: 'minecraft' }),
    ];
    expect(compatSummaryFromScan(scan, 1)).toContain('Mods tab');
  });
});
