import { beforeEach, describe, expect, it } from 'vitest';
import {
  ALL_CONTEXTUAL_TOUR_IDS,
  hasSeen,
  markSeen,
  resetAllContextualTours,
  storageKey,
} from '../src/lib/onboarding/contextual-tours';

describe('contextual-tours storage helpers', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('storageKey produces a versioned key per id', () => {
    // Version suffix lets us invalidate the dismissed flag when steps
    // change materially (logs got v2 in 2026-05-26; v3 in 2026-06-15 — header redesign moved Share into the ⋯ menu;
    // manage got v2 in 2026-06-23 — added the Verify/repair step).
    expect(storageKey('manage')).toBe('ftl.tour.manage.v2.done');
    expect(storageKey('logs')).toBe('ftl.tour.logs.v3.done');
    expect(storageKey('modpacks')).toBe('ftl.tour.modpacks.v1.done');
    expect(storageKey('worlds')).toBe('ftl.tour.worlds.v2.done');
    expect(storageKey('servers')).toBe('ftl.tour.servers.v1.done');
    expect(storageKey('serverManage')).toBe('ftl.tour.serverManage.v1.done');
  });

  it('hasSeen returns false on fresh storage', () => {
    expect(hasSeen('manage')).toBe(false);
    expect(hasSeen('logs')).toBe(false);
    expect(hasSeen('modpacks')).toBe(false);
    expect(hasSeen('worlds')).toBe(false);
  });

  it('markSeen persists, hasSeen reads it', () => {
    markSeen('manage');
    expect(hasSeen('manage')).toBe(true);
    expect(hasSeen('logs')).toBe(false);
    expect(hasSeen('modpacks')).toBe(false);
    expect(hasSeen('worlds')).toBe(false);
  });

  it("hasSeen ignores values other than '1'", () => {
    localStorage.setItem(storageKey('logs'), '0');
    expect(hasSeen('logs')).toBe(false);
    localStorage.setItem(storageKey('logs'), 'true');
    expect(hasSeen('logs')).toBe(false);
  });

  it('ALL_CONTEXTUAL_TOUR_IDS covers every tour', () => {
    expect([...ALL_CONTEXTUAL_TOUR_IDS].sort()).toEqual(
      ['logs', 'manage', 'modpacks', 'serverManage', 'servers', 'worlds'].sort(),
    );
  });

  it('resetAllContextualTours clears every seen flag', () => {
    for (const id of ALL_CONTEXTUAL_TOUR_IDS) markSeen(id);
    for (const id of ALL_CONTEXTUAL_TOUR_IDS) expect(hasSeen(id)).toBe(true);

    resetAllContextualTours();

    for (const id of ALL_CONTEXTUAL_TOUR_IDS) expect(hasSeen(id)).toBe(false);
  });
});
