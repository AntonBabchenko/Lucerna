import { describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: vi.fn(async () => ({ status: 'error', error: 'unused' })),
    appSettingsMarkTourCompleted: vi.fn(async () => ({ status: 'ok', data: null })),
  },
}));

import {
  ALL_CONTEXTUAL_TOUR_IDS,
  fingerprintSteps,
  STEPS_BY_ID,
  STEPS_FINGERPRINT,
} from '../src/lib/onboarding/contextual-tours';
import { MAIN_STEPS_FINGERPRINT } from '../src/lib/onboarding/state.svelte';
import { STEPS } from '../src/lib/onboarding/steps';

// The bump nudge (#294): editing a tour's steps must force an edit on the
// fingerprint line that sits BESIDE the TOUR_VERSION line, so author and
// reviewer both face the "does this need a version bump?" question.
describe('tour steps fingerprints', () => {
  for (const id of ALL_CONTEXTUAL_TOUR_IDS) {
    it(`${id}: recorded fingerprint matches the steps`, () => {
      expect(
        STEPS_FINGERPRINT[id],
        `steps for "${id}" changed: set STEPS_FINGERPRINT.${id} to "${fingerprintSteps(STEPS_BY_ID[id])}" AND decide whether TOUR_VERSION.${id} must bump (see the bump contract comment)`,
      ).toBe(fingerprintSteps(STEPS_BY_ID[id]));
    });
  }

  it('main tour: recorded fingerprint matches STEPS', () => {
    expect(
      MAIN_STEPS_FINGERPRINT,
      `main STEPS changed: set MAIN_STEPS_FINGERPRINT to "${fingerprintSteps(STEPS)}" AND decide whether the main TOUR_VERSION must bump`,
    ).toBe(fingerprintSteps(STEPS));
  });
});
