import { describe, expect, it } from 'vitest';
import { shouldFocusAnchor } from '$lib/settings/search-index';

describe('shouldFocusAnchor', () => {
  it('does not grab focus for a select-style anchor', () => {
    expect(shouldFocusAnchor('game.gpu')).toBe(false);
  });

  it('does not grab focus for a toggle-style anchor', () => {
    expect(shouldFocusAnchor('game.tray')).toBe(false);
  });
});
