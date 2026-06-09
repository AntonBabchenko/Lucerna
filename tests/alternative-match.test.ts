import { describe, expect, it } from 'vitest';
import { isLikelyMatch, normalizeModName } from '$lib/mods/alternative-match';

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
