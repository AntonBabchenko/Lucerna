import { describe, expect, it } from 'vitest';
import { L10N_STEPS, OVERVIEW_STEPS } from '../src/lib/onboarding/contextual-tours';

// Structural guard for the mod-localization tour and the single-step Overview
// tour that points at the localization row. Keeps the step definitions and the
// DOM anchors they target from drifting apart.

describe('l10n + overview tours', () => {
  it('l10n tour anchors coverage list, find-string row, header actions, in order', () => {
    expect(L10N_STEPS.map((s) => s.targetSelector)).toEqual([
      '[data-tour-ctx="l10n-coverage"]',
      '[data-tour-ctx="l10n-search"]',
      '[data-tour-ctx="l10n-actions"]',
    ]);
  });

  it('overview tour is a single step on the localization row', () => {
    expect(OVERVIEW_STEPS.map((s) => s.targetSelector)).toEqual([
      '[data-tour-ctx="overview-l10n"]',
    ]);
  });
});
