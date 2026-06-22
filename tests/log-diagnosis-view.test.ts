import { describe, expect, it } from 'vitest';
import type { LatestDiagnosis } from '$lib/ipc/bindings';
import {
  inlineDiagnosisRedundant,
  logBannerEligible,
  logDiagnosisSignature,
} from '$lib/logs/log-diagnosis-view';

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

describe('inlineDiagnosisRedundant', () => {
  const SELECTED = '/logs/latest.log'; // matches latest().path

  it('is true when the visible banner is for the selected file', () => {
    expect(inlineDiagnosisRedundant(SELECTED, latest(), false)).toBe(true);
  });

  it('is false when a different (older) file is selected', () => {
    expect(inlineDiagnosisRedundant('/logs/2026-06-20.log.gz', latest(), false)).toBe(false);
  });

  it('is false when the banner is dismissed (no banner above to dedupe against)', () => {
    expect(inlineDiagnosisRedundant(SELECTED, latest(), true)).toBe(false);
  });

  it('is false when the banner is not eligible (status/no diagnosis)', () => {
    expect(inlineDiagnosisRedundant(SELECTED, latest({ status: 'none' }), false)).toBe(false);
    expect(inlineDiagnosisRedundant(SELECTED, latest({ diagnosis: null }), false)).toBe(false);
  });

  it('is false when nothing is selected or there is no latest diagnosis', () => {
    expect(inlineDiagnosisRedundant(null, latest(), false)).toBe(false);
    expect(inlineDiagnosisRedundant(SELECTED, null, false)).toBe(false);
  });

  it('is false when the latest diagnosis has a null path', () => {
    expect(inlineDiagnosisRedundant(SELECTED, latest({ path: null }), false)).toBe(false);
  });
});
