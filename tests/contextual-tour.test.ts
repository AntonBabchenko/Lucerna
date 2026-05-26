import { beforeEach, describe, expect, it } from 'vitest';
import { hasSeen, markSeen, storageKey } from '../src/lib/onboarding/contextual-tours';

describe('contextual-tours storage helpers', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('storageKey produces predictable key for each id', () => {
    expect(storageKey('manage')).toBe('ftl.tour.manage.done');
    expect(storageKey('logs')).toBe('ftl.tour.logs.done');
    expect(storageKey('modpacks')).toBe('ftl.tour.modpacks.done');
  });

  it('hasSeen returns false on fresh storage', () => {
    expect(hasSeen('manage')).toBe(false);
    expect(hasSeen('logs')).toBe(false);
    expect(hasSeen('modpacks')).toBe(false);
  });

  it('markSeen persists, hasSeen reads it', () => {
    markSeen('manage');
    expect(hasSeen('manage')).toBe(true);
    expect(hasSeen('logs')).toBe(false);
    expect(hasSeen('modpacks')).toBe(false);
  });

  it("hasSeen ignores values other than '1'", () => {
    localStorage.setItem(storageKey('logs'), '0');
    expect(hasSeen('logs')).toBe(false);
    localStorage.setItem(storageKey('logs'), 'true');
    expect(hasSeen('logs')).toBe(false);
  });
});
