import { describe, expect, it } from 'vitest';
import {
  createBlankSkin,
  getTexel,
  restore,
  SKIN_SIZE,
  setTexel,
  snapshot,
  validateSkinDimensions,
} from '$lib/accounts/skin-editor/buffer';

describe('buffer', () => {
  it('creates a fully transparent 64x64 buffer', () => {
    const d = createBlankSkin();
    expect(d.length).toBe(SKIN_SIZE * SKIN_SIZE * 4);
    expect([...d.slice(0, 4)]).toEqual([0, 0, 0, 0]);
  });

  it('sets and gets a texel', () => {
    const d = createBlankSkin();
    setTexel(d, 3, 5, [10, 20, 30, 255]);
    expect(getTexel(d, 3, 5)).toEqual([10, 20, 30, 255]);
    expect(getTexel(d, 0, 0)).toEqual([0, 0, 0, 0]);
  });

  it('ignores out-of-range coordinates', () => {
    const d = createBlankSkin();
    setTexel(d, -1, 0, [1, 2, 3, 4]);
    setTexel(d, 64, 0, [1, 2, 3, 4]);
    expect([...d].every((v) => v === 0)).toBe(true);
    expect(getTexel(d, 64, 64)).toEqual([0, 0, 0, 0]);
  });

  it('snapshots and restores independently of the live buffer', () => {
    const d = createBlankSkin();
    setTexel(d, 1, 1, [9, 9, 9, 255]);
    const snap = snapshot(d);
    setTexel(d, 1, 1, [1, 1, 1, 255]);
    expect(getTexel(d, 1, 1)).toEqual([1, 1, 1, 255]);
    restore(d, snap);
    expect(getTexel(d, 1, 1)).toEqual([9, 9, 9, 255]);
  });

  it('validates skin dimensions', () => {
    expect(validateSkinDimensions(64, 64)).toBe('ok');
    expect(validateSkinDimensions(64, 32)).toBe('legacy');
    expect(validateSkinDimensions(32, 32)).toBe('invalid');
    expect(validateSkinDimensions(128, 128)).toBe('invalid');
  });
});
