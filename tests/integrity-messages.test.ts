import { beforeEach, describe, expect, it } from 'vitest';
import { locale } from '$lib/i18n';
import { compatSummary, loaderOutcomeToast } from '$lib/instances/integrity-messages';
import type { LoaderOutcome, ModCompat } from '$lib/ipc/bindings';

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
// compatSummary
// ---------------------------------------------------------------------------

function makeCompat(status: ModCompat['status']): ModCompat {
  return { sha1: 'abc', name: 'TestMod', status };
}

describe('compatSummary', () => {
  it('returns null for empty array', () => {
    expect(compatSummary([])).toBeNull();
  });

  it('returns null when all mods are compatible', () => {
    const rows: ModCompat[] = [
      makeCompat({ status: 'compatible', available_version: '1.0' }),
      makeCompat({ status: 'compatible', available_version: null }),
    ];
    expect(compatSummary(rows)).toBeNull();
  });

  it('returns a string with correct incompatible count when some are incompatible', () => {
    const rows: ModCompat[] = [
      makeCompat({ status: 'compatible', available_version: '1.0' }),
      makeCompat({ status: 'incompatible' }),
      makeCompat({ status: 'incompatible' }),
    ];
    const result = compatSummary(rows);
    expect(result).not.toBeNull();
    expect(result).toContain('2 of 3');
  });

  it('does not include unknown suffix when unknown count is 0', () => {
    const rows: ModCompat[] = [
      makeCompat({ status: 'incompatible' }),
      makeCompat({ status: 'compatible', available_version: '2.0' }),
    ];
    const result = compatSummary(rows);
    expect(result).not.toBeNull();
    expect(result).not.toContain("couldn't be checked");
  });

  it('includes unknown suffix when unknown count > 0', () => {
    const rows: ModCompat[] = [
      makeCompat({ status: 'incompatible' }),
      makeCompat({ status: 'unknown' }),
      makeCompat({ status: 'compatible', available_version: '1.0' }),
    ];
    const result = compatSummary(rows);
    expect(result).not.toBeNull();
    expect(result).toContain("1 couldn't be checked");
  });

  it('counts only unknowns (no incompatible) as worth surfacing', () => {
    const rows: ModCompat[] = [
      makeCompat({ status: 'unknown' }),
      makeCompat({ status: 'compatible', available_version: '1.0' }),
    ];
    const result = compatSummary(rows);
    // 0 incompatible but 1 unknown — still show warning (user should know)
    expect(result).not.toBeNull();
    expect(result).toContain("couldn't be checked");
  });

  it('mentions review in mods tab', () => {
    const rows: ModCompat[] = [makeCompat({ status: 'incompatible' })];
    expect(compatSummary(rows)).toContain('Mods tab');
  });
});
