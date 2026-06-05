import { describe, expect, it } from 'vitest';
import { effectiveIntegrityStatus, isIntegrityStale } from '$lib/instances/integrity-freshness';
import type { IntegrityStatus } from '$lib/ipc/bindings';

// A fixed "session start" so freshness is deterministic regardless of the
// runner's clock. checked_unix_ms values are chosen relative to this.
const SESSION = 1_000_000;

function makeStatus(p: Partial<IntegrityStatus> = {}): IntegrityStatus {
  return { healthy: true, checked_unix_ms: SESSION, categories: [], problem_count: 0, ...p };
}

describe('isIntegrityStale', () => {
  it('returns false for null/undefined', () => {
    expect(isIntegrityStale(null, SESSION)).toBe(false);
    expect(isIntegrityStale(undefined, SESSION)).toBe(false);
  });

  it('treats a healthy result checked before this session as stale', () => {
    const status = makeStatus({ healthy: true, checked_unix_ms: SESSION - 1 });
    expect(isIntegrityStale(status, SESSION)).toBe(true);
  });

  it('treats a healthy result checked during this session as fresh', () => {
    expect(isIntegrityStale(makeStatus({ checked_unix_ms: SESSION }), SESSION)).toBe(false);
    expect(isIntegrityStale(makeStatus({ checked_unix_ms: SESSION + 5 }), SESSION)).toBe(false);
  });

  it('never marks a problem result stale, even from a previous session', () => {
    const status = makeStatus({ healthy: false, problem_count: 3, checked_unix_ms: SESSION - 999 });
    expect(isIntegrityStale(status, SESSION)).toBe(false);
  });

  it('treats a healthy result with no timestamp as fresh (cannot prove stale)', () => {
    expect(isIntegrityStale(makeStatus({ checked_unix_ms: null }), SESSION)).toBe(false);
  });
});

describe('effectiveIntegrityStatus', () => {
  it('returns null for null/undefined input', () => {
    expect(effectiveIntegrityStatus(null, SESSION)).toBeNull();
    expect(effectiveIntegrityStatus(undefined, SESSION)).toBeNull();
  });

  it('returns null for a stale healthy result (renders as not-checked)', () => {
    const status = makeStatus({ healthy: true, checked_unix_ms: SESSION - 1 });
    expect(effectiveIntegrityStatus(status, SESSION)).toBeNull();
  });

  it('returns the status unchanged for a fresh healthy result', () => {
    const status = makeStatus({ checked_unix_ms: SESSION + 1 });
    expect(effectiveIntegrityStatus(status, SESSION)).toBe(status);
  });

  it('returns the status unchanged for a problem result regardless of age', () => {
    const status = makeStatus({ healthy: false, problem_count: 2, checked_unix_ms: SESSION - 500 });
    expect(effectiveIntegrityStatus(status, SESSION)).toBe(status);
  });
});
