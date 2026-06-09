import { describe, expect, it } from 'vitest';
import { isUnresolvedMissingState } from '$lib/modpacks/missing-mod';

describe('isUnresolvedMissingState', () => {
  it('treats missing and different_version as unresolved', () => {
    expect(isUnresolvedMissingState('missing')).toBe(true);
    expect(isUnresolvedMissingState('different_version')).toBe(true);
  });

  it('treats installed and substituted as resolved', () => {
    expect(isUnresolvedMissingState('installed')).toBe(false);
    expect(isUnresolvedMissingState('substituted')).toBe(false);
  });
});
