import { describe, expect, it } from 'vitest';
import { SERVER_MANAGE_STEPS, SERVERS_STEPS } from '../src/lib/onboarding/contextual-tours';

// Structural guards for the two Servers contextual tours. They keep the step
// definitions and the DOM anchors they target from drifting apart silently —
// the host components carry the matching data-tour-ctx attributes.

function selector(step: { targetSelector: string | null }): string {
  return step.targetSelector ?? '';
}

describe('servers list tour (SERVERS_STEPS)', () => {
  it('has the three list-view steps in order', () => {
    expect(SERVERS_STEPS.map(selector)).toEqual([
      '[data-tour-ctx="servers-create"]',
      '[data-tour-ctx="servers-list"]',
      '[data-tour-ctx="servers-lan"]',
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
  it('has the six detail-view steps in order, anchored to stable elements', () => {
    expect(SERVER_MANAGE_STEPS.map(selector)).toEqual([
      '[data-tour-ctx="server-header-actions"]',
      '[data-tour-ctx="server-tab-console"]',
      '[data-tour-ctx="server-tab-mods"]',
      '[data-tour-ctx="server-tab-connect"]',
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
