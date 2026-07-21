import { describe, expect, it } from 'vitest';
import type { Rgba } from '$lib/accounts/skin-editor/buffer';
import {
  addSwatch,
  DEFAULT_PALETTE,
  MAX_SWATCHES,
  moveSwatch,
  normalizePalette,
  parseHexColour,
  removeSwatch,
  replaceSwatch,
  serializePalette,
} from '$lib/accounts/skin-editor/palette';

const RED: Rgba = [255, 0, 0, 255];
const GREEN: Rgba = [0, 255, 0, 255];

describe('palette operations', () => {
  it('DEFAULT_PALETTE has 10 opaque swatches', () => {
    expect(DEFAULT_PALETTE).toHaveLength(10);
    expect(DEFAULT_PALETTE.every((c) => c[3] === 255)).toBe(true);
  });

  it('addSwatch appends an opaque copy without mutating the input', () => {
    const base: Rgba[] = [RED];
    const next = addSwatch(base, [10, 20, 30, 0]);
    expect(next).toEqual([RED, [10, 20, 30, 255]]);
    expect(base).toEqual([RED]); // unchanged
  });

  it('addSwatch is a no-op at MAX_SWATCHES', () => {
    const full: Rgba[] = Array.from({ length: MAX_SWATCHES }, () => RED);
    expect(addSwatch(full, GREEN)).toHaveLength(MAX_SWATCHES);
  });

  it('removeSwatch drops the given index and ignores out-of-range', () => {
    expect(removeSwatch([RED, GREEN], 0)).toEqual([GREEN]);
    expect(removeSwatch([RED], 5)).toEqual([RED]);
  });

  it('replaceSwatch swaps in place (opaque); ignores out-of-range', () => {
    expect(replaceSwatch([RED, GREEN], 1, [1, 2, 3, 0])).toEqual([RED, [1, 2, 3, 255]]);
    expect(replaceSwatch([RED], 9, GREEN)).toEqual([RED]);
  });

  it('moveSwatch reorders and clamps indices', () => {
    expect(moveSwatch([RED, GREEN], 0, 1)).toEqual([GREEN, RED]);
    expect(moveSwatch([RED, GREEN], 0, 99)).toEqual([GREEN, RED]); // clamp to last
    expect(moveSwatch([RED, GREEN], 0, 0)).toEqual([RED, GREEN]); // equal -> no move
    expect(moveSwatch([RED], 0, 0)).toEqual([RED]); // single -> no-op
  });
});

describe('palette serialize / parse', () => {
  it('parseHexColour parses #rrggbb (case-insensitive) to opaque Rgba', () => {
    expect(parseHexColour('#ff8000')).toEqual([255, 128, 0, 255]);
    expect(parseHexColour('#AABBCC')).toEqual([170, 187, 204, 255]);
  });

  it('parseHexColour rejects malformed input', () => {
    expect(parseHexColour('#fff')).toBeNull(); // shorthand not accepted
    expect(parseHexColour('ff8000')).toBeNull(); // missing #
    expect(parseHexColour('#gg0000')).toBeNull();
    expect(parseHexColour(42)).toBeNull();
    expect(parseHexColour(null)).toBeNull();
  });

  it('serializePalette round-trips through normalizePalette', () => {
    const list: Rgba[] = [
      [255, 0, 0, 255],
      [0, 128, 255, 255],
    ];
    expect(serializePalette(list)).toEqual(['#ff0000', '#0080ff']);
    expect(normalizePalette(serializePalette(list))).toEqual(list);
  });

  it('normalizePalette falls back to defaults for a non-array', () => {
    expect(normalizePalette(null)).toEqual(DEFAULT_PALETTE);
    expect(normalizePalette('{bad')).toEqual(DEFAULT_PALETTE);
    expect(normalizePalette(undefined)).toEqual(DEFAULT_PALETTE);
  });

  it('normalizePalette keeps a valid empty array empty (user cleared it)', () => {
    expect(normalizePalette([])).toEqual([]);
  });

  it('normalizePalette drops malformed entries and caps at MAX_SWATCHES', () => {
    expect(normalizePalette(['#ff0000', 'nope', 123, '#00ff00'])).toEqual([
      [255, 0, 0, 255],
      [0, 255, 0, 255],
    ]);
    const overflow = Array.from({ length: MAX_SWATCHES + 5 }, () => '#010203');
    expect(normalizePalette(overflow)).toHaveLength(MAX_SWATCHES);
  });

  it('normalizePalette returns fresh copies (not shared with DEFAULT_PALETTE)', () => {
    const out = normalizePalette(null);
    out[0][0] = 1;
    expect(DEFAULT_PALETTE[0][0]).toBe(224); // unchanged
  });
});
