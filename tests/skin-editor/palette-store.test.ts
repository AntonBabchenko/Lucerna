import { beforeEach, describe, expect, it } from 'vitest';
import { DEFAULT_PALETTE } from '$lib/accounts/skin-editor/palette';
import { loadPalette } from '$lib/accounts/skin-editor/palette.svelte';

const KEY = 'lucerna.skinPalette';

describe('skin palette store — loadPalette', () => {
  beforeEach(() => localStorage.clear());

  it('defaults to DEFAULT_PALETTE when nothing is stored', () => {
    expect(loadPalette()).toEqual(DEFAULT_PALETTE);
  });

  it('reads a valid persisted palette', () => {
    localStorage.setItem(KEY, JSON.stringify(['#ff0000', '#00ff00']));
    expect(loadPalette()).toEqual([
      [255, 0, 0, 255],
      [0, 255, 0, 255],
    ]);
  });

  it('falls back to defaults on malformed JSON', () => {
    localStorage.setItem(KEY, '{not json');
    expect(loadPalette()).toEqual(DEFAULT_PALETTE);
  });

  it('respects a deliberately cleared (empty) palette', () => {
    localStorage.setItem(KEY, JSON.stringify([]));
    expect(loadPalette()).toEqual([]);
  });
});
