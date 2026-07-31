import { describe, expect, it } from 'vitest';
import { SERVER_MANAGE_STEPS, SERVERS_STEPS } from '../src/lib/onboarding/contextual-tours';

// Structural guards for the two Servers contextual tours. They keep the step
// definitions and the DOM anchors they target from drifting apart silently —
// the host components carry the matching data-tour-ctx attributes.

function selector(step: { targetSelector: string | null }): string {
  return step.targetSelector ?? '';
}

describe('servers mode tour (SERVERS_STEPS)', () => {
  it('has the two servers-mode steps in order', () => {
    expect(SERVERS_STEPS.map(selector)).toEqual([
      '[data-tour-ctx="servers-create"]',
      '[data-tour-ctx="servers-mode-switch"]',
    ]);
  });

  it('every step carries a title + body key and a non-null selector', () => {
    for (const step of SERVERS_STEPS) {
      expect(step.titleKey).toBeTruthy();
      expect(step.bodyKey).toBeTruthy();
      expect(step.targetSelector).not.toBeNull();
    }
  });
});

describe('server manage tour (SERVER_MANAGE_STEPS)', () => {
  it('has the five detail-view steps in order, anchored to stable elements', () => {
    expect(SERVER_MANAGE_STEPS.map(selector)).toEqual([
      // Start/Stop is a SIDEBAR control; anchoring this step to the panel
      // header made it spotlight "create client instance" instead.
      '[data-tour-ctx="server-start-stop"]',
      '[data-tour-ctx="server-tab-overview"]',
      '[data-tour-ctx="server-tab-addons"]',
      '[data-tour-ctx="server-tab-hosting"]',
      '[data-tour-ctx="server-to-instance"]',
    ]);
  });

  it('every step carries a title + body key and a non-null selector', () => {
    for (const step of SERVER_MANAGE_STEPS) {
      expect(step.titleKey).toBeTruthy();
      expect(step.bodyKey).toBeTruthy();
      expect(step.targetSelector).not.toBeNull();
    }
  });
});
