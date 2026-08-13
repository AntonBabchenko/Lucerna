import { describe, expect, it } from 'vitest';
import { SERVER_ADDONS_STEPS } from '../src/lib/onboarding/contextual-tours';

// Structural guard for the server Add-ons tab tour. Keeps the step definitions
// and the DOM anchors they target (in ServerAddonsTab.svelte) from drifting
// apart.

describe('server add-ons tour (SERVER_ADDONS_STEPS)', () => {
  it('anchors the kind switch and the dropzone, in order', () => {
    expect(SERVER_ADDONS_STEPS.map((s) => s.targetSelector)).toEqual([
      '[data-tour-ctx="server-addons-kind-switch"]',
      '[data-tour-ctx="server-addons-dropzone"]',
    ]);
  });

  it('every step carries a title + body key', () => {
    for (const step of SERVER_ADDONS_STEPS) {
      expect(step.titleKey).toBeTruthy();
      expect(step.bodyKey).toBeTruthy();
    }
  });
});
