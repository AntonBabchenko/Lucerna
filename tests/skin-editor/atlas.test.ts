import { describe, expect, it } from 'vitest';
import { allFaceRects, faceRectAt, mirrorInFace } from '$lib/accounts/skin-editor/atlas';

describe('atlas', () => {
  it('finds the head-front base face at (8..15, 8..15)', () => {
    const r = faceRectAt(10, 10, 'classic');
    expect(r).toEqual({ x: 8, y: 8, w: 8, h: 8, part: 'head', layer: 'base', face: 'front' });
  });

  it('finds the hat (head overlay) front face at (40..47, 8..15)', () => {
    const r = faceRectAt(42, 10, 'classic');
    expect(r?.part).toBe('head');
    expect(r?.layer).toBe('overlay');
  });

  it('returns undefined for an unused atlas cell', () => {
    expect(faceRectAt(60, 2, 'classic')).toBeUndefined();
  });

  it('every rect stays inside the 64x64 atlas for both variants', () => {
    for (const variant of ['classic', 'slim'] as const) {
      for (const r of allFaceRects(variant)) {
        expect(r.x).toBeGreaterThanOrEqual(0);
        expect(r.y).toBeGreaterThanOrEqual(0);
        expect(r.x + r.w).toBeLessThanOrEqual(64);
        expect(r.y + r.h).toBeLessThanOrEqual(64);
      }
    }
  });

  it('base rects never overlap each other (classic)', () => {
    const rects = allFaceRects('classic');
    for (let i = 0; i < rects.length; i++) {
      for (let j = i + 1; j < rects.length; j++) {
        const a = rects[i];
        const b = rects[j];
        const overlap = a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
        expect(
          overlap,
          `${a.part}/${a.layer}/${a.face} overlaps ${b.part}/${b.layer}/${b.face}`,
        ).toBe(false);
      }
    }
  });

  it('mirrors a texel horizontally within its face', () => {
    // head-front rect is x:8 w:8 -> mirror of x=8 is x=15, of x=9 is x=14
    expect(mirrorInFace(8, 10, 'classic')).toEqual({ x: 15, y: 10 });
    expect(mirrorInFace(9, 10, 'classic')).toEqual({ x: 14, y: 10 });
  });

  it('returns the same texel when it is outside any face (no-op mirror)', () => {
    expect(mirrorInFace(60, 2, 'classic')).toEqual({ x: 60, y: 2 });
  });

  it('slim arm front face is 3 wide', () => {
    const r = allFaceRects('slim').find(
      (f) => f.part === 'rightArm' && f.layer === 'base' && f.face === 'front',
    );
    expect(r?.w).toBe(3);
  });
});
