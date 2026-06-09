import { describe, expect, it } from 'vitest';
import { deriveSearchQuery, isLikelyMatch, normalizeModName } from '$lib/mods/alternative-match';

describe('normalizeModName', () => {
  it('lowercases and strips non-alphanumerics', () => {
    expect(normalizeModName('Create: Train Parts!')).toBe('createtrainparts');
    expect(normalizeModName('create_train_parts-0.4.0')).toBe('createtrainparts040');
  });
});

describe('isLikelyMatch', () => {
  it('matches names that normalize equally despite punctuation/case', () => {
    expect(isLikelyMatch('Create Train Parts', 'create-train-parts')).toBe(true);
  });

  it('does not match clearly different names', () => {
    expect(isLikelyMatch('Create Train Parts', 'Sodium')).toBe(false);
  });

  it('matches when one normalized name contains the other', () => {
    expect(isLikelyMatch('JEI', 'JEI (Just Enough Items)')).toBe(true);
  });

  it('does not match on a too-short (<3 char) substring', () => {
    expect(isLikelyMatch('AB', 'something AB here')).toBe(false);
  });
});

describe('deriveSearchQuery', () => {
  it('strips extension and version/mc tokens from a jar filename', () => {
    expect(deriveSearchQuery('moreoverlays-1.21.5-mc1.19.2.jar')).toBe('moreoverlays');
    expect(deriveSearchQuery('hexerei-0.3.0.jar')).toBe('hexerei');
    expect(deriveSearchQuery('appleskin-mc1.19-2.4.0.jar')).toBe('appleskin');
  });

  it('turns separators into spaces for multi-word mod ids', () => {
    expect(deriveSearchQuery('create_train_parts-0.4.0-1.21.1-6.0.9-216.jar')).toBe(
      'create train parts',
    );
  });

  it('passes a clean display name through unchanged', () => {
    expect(deriveSearchQuery('More Overlays Updated')).toBe('More Overlays Updated');
    expect(deriveSearchQuery('JEI')).toBe('JEI');
  });

  it('falls back to the cleaned stem when every token looks like junk', () => {
    // A degenerate loader-ish entry — better to search something than nothing.
    expect(deriveSearchQuery('1.11.1-1.21.1.jar')).toBe('1.11.1 1.21.1');
  });
});
