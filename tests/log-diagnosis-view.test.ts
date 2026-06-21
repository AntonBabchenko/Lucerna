import { describe, expect, it } from 'vitest';
import type { LatestDiagnosis } from '$lib/ipc/bindings';
import { logBannerEligible, logDiagnosisSignature } from '$lib/logs/log-diagnosis-view';

function latest(overrides: Partial<LatestDiagnosis> = {}): LatestDiagnosis {
  return {
    status: 'actionable',
    diagnosis: {
      pattern_id: 'missing-mods',
      title: 't',
      explanation: 'e',
      recommendation: 'r',
      matched_excerpt: '',
      repair: null,
    },
    path: '/logs/latest.log',
    signature: 'log-sig-1',
    ...overrides,
  };
}

describe('logBannerEligible', () => {
  it('is true for actionable and advisory', () => {
    expect(logBannerEligible(latest({ status: 'actionable' }))).toBe(true);
    expect(logBannerEligible(latest({ status: 'advisory' }))).toBe(true);
  });

  it('is false for none / handled / missing diagnosis / null', () => {
    expect(logBannerEligible(latest({ status: 'none' }))).toBe(false);
    expect(logBannerEligible(latest({ status: 'handled' }))).toBe(false);
    expect(logBannerEligible(latest({ diagnosis: null }))).toBe(false);
    expect(logBannerEligible(null)).toBe(false);
  });
});

describe('logDiagnosisSignature', () => {
  it('prefers the log content signature', () => {
    expect(logDiagnosisSignature(latest())).toBe('log-sig-1');
  });

  it('falls back to the pattern id when signature is null', () => {
    expect(logDiagnosisSignature(latest({ signature: null }))).toBe('missing-mods');
  });

  it('is null when there is no diagnosis', () => {
    expect(logDiagnosisSignature(latest({ diagnosis: null }))).toBeNull();
    expect(logDiagnosisSignature(null)).toBeNull();
  });
});
