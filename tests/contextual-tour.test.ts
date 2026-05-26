import { beforeEach, describe, expect, it } from 'vitest';
import { hasSeen, markSeen, storageKey } from '../src/lib/onboarding/contextual-tours';

describe('contextual-tours storage helpers', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('storageKey produces a versioned key per id', () => {
    // Version suffix lets us invalidate the dismissed flag when steps
    // change materially (logs got v2 in 2026-05-26).
    expect(storageKey('manage')).toBe('ftl.tour.manage.v1.done');
    expect(storageKey('logs')).toBe('ftl.tour.logs.v2.done');
    expect(storageKey('modpacks')).toBe('ftl.tour.modpacks.v1.done');
    expect(storageKey('worlds')).toBe('ftl.tour.worlds.v2.done');
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
});
