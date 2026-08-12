import { describe, expect, it } from 'vitest';
import { ADDONS_STEPS } from '../src/lib/onboarding/contextual-tours';

// Structural guard for the Add-ons tab layout tour. Keeps the step definitions
// and the DOM anchors they target (in AddonsTab.svelte) from drifting apart.

describe('add-ons tab tour (ADDONS_STEPS)', () => {
  it('has the four layout steps in order', () => {
    // The kind-switch selector appears TWICE on purpose: the datapack-library
    // step rides the same anchor as the kind switch, because the datapack tab
    // body only exists once that kind is selected. Precedent: the logs tour
    // anchors three separate steps to `logs-sidebar`. Do not "fix" the
    // duplicate — deleting it silently drops a step from the tour.
    expect(ADDONS_STEPS.map((s) => s.targetSelector)).toEqual([
      '[data-tour-ctx="addons-kind-switch"]',
      '[data-tour-ctx="addons-kind-switch"]',
      '[data-tour-ctx="addons-subtabs"]',
      '[data-tour-ctx="addons-dropzone"]',
    ]);
  });

  it('every step carries a title + body key and a non-null selector', () => {
    for (const step of ADDONS_STEPS) {
      expect(step.titleKey).toBeTruthy();
      expect(step.bodyKey).toBeTruthy();
      expect(step.targetSelector).not.toBeNull();
    }
  });
});
