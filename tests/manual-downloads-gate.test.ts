import { describe, expect, it } from 'vitest';
import type { DepViolation, PreflightReport } from '$lib/ipc/bindings';
import { decideLaunch, hasBlocking } from '$lib/mods/preflight.svelte';
import { isFixable, violationAction } from '$lib/mods/violation-view';

const missing = { kind: 'missing_required', dep_id: 'srp' } as unknown as DepViolation;
const empty: PreflightReport = { violations: [], unjudged: [] } as unknown as PreflightReport;

describe('pending manual downloads at Play', () => {
  it('opens the gate even with no violation', () => {
    const report = { ...empty, manual_pending: [{ key: 'file:1', mod_name: 'SRP' }] };
    expect(decideLaunch({ status: 'ok', data: report })).toEqual({ kind: 'gate', report });
  });
  it('launches when nothing is pending and nothing blocks', () => {
    expect(decideLaunch({ status: 'ok', data: empty })).toEqual({ kind: 'launch' });
  });
  it('offers no automatic fix for a missing dependency while files are pending', () => {
    expect(violationAction(missing, true)).toBe('none');
    expect(isFixable(missing, true)).toBe(false);
    expect(violationAction(missing)).toBe('install');
  });
  it('leaves hasBlocking alone', () => {
    expect(hasBlocking({ ...empty, manual_pending: [{ key: 'k', mod_name: 'M' }] })).toBe(false);
  });
});
