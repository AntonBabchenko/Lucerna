import { describe, expect, it } from 'vitest';
import {
  allFaceRects,
  faceRectAt,
  mirrorBlockAnchor,
  mirrorInFace,
  mirrorTexel,
} from '$lib/accounts/skin-editor/atlas';

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

describe('atlas mirror (sagittal x=0)', () => {
  it('is an involution over every texel of every rect (both variants, both layers)', () => {
    for (const variant of ['classic', 'slim'] as const) {
      for (const r of allFaceRects(variant)) {
        for (let y = r.y; y < r.y + r.h; y++) {
          for (let x = r.x; x < r.x + r.w; x++) {
            const m = mirrorTexel(x, y, variant);
            if (!m) throw new Error(`no mirror for ${x},${y} in ${r.part}/${r.layer}/${r.face}`);
            expect(mirrorTexel(m.x, m.y, variant)).toEqual({ x, y });
          }
        }
      }
    }
  });

  it('keeps head-front on itself with U flipped (left cheek -> right cheek)', () => {
    // head-front base rect is x:8 w:8 -> local x=0 mirrors to local x=7
    expect(mirrorTexel(8, 8, 'classic')).toEqual({ x: 15, y: 8 });
    expect(faceRectAt(15, 8, 'classic')).toMatchObject({
      part: 'head',
      layer: 'base',
      face: 'front',
    });
  });

  it('swaps head left face to head right face (left ear -> right ear)', () => {
    // head-left base = (0,8,8,8); head-right base = (16,8,8,8)
    expect(mirrorTexel(0, 8, 'classic')).toEqual({ x: 23, y: 8 });
    expect(faceRectAt(23, 8, 'classic')).toMatchObject({
      part: 'head',
      layer: 'base',
      face: 'right',
    });
  });

  it('swaps right arm to left arm cross-limb, preserving layer (classic)', () => {
    // rightArm-front base = (44,20,4,12); leftArm-front base = (36,52,4,12)
    expect(mirrorTexel(44, 20, 'classic')).toEqual({ x: 39, y: 52 });
    expect(faceRectAt(39, 52, 'classic')).toMatchObject({
      part: 'leftArm',
      layer: 'base',
      face: 'front',
    });
  });

  it('swaps right leg overlay to left leg overlay (layer preserved)', () => {
    const src = allFaceRects('classic').find(
      (f) => f.part === 'rightLeg' && f.layer === 'overlay' && f.face === 'front',
    );
    if (!src) throw new Error('rightLeg overlay front rect missing');
    const m = mirrorTexel(src.x, src.y, 'classic');
    if (!m) throw new Error('no mirror for rightLeg overlay front');
    expect(faceRectAt(m.x, m.y, 'classic')).toMatchObject({
      part: 'leftLeg',
      layer: 'overlay',
      face: 'front',
    });
  });

  it('maps the odd slim-arm center column to the OTHER arm center, not itself', () => {
    // slim rightArm-front = (44,20,3,12) center col x=45; leftArm-front = (36,52,3,12) center x=37
    expect(mirrorTexel(45, 20, 'slim')).toEqual({ x: 37, y: 52 });
  });

  it('returns null when the texel is outside every face', () => {
    expect(mirrorTexel(60, 2, 'classic')).toBeNull();
  });

  it('anchors a mirrored brush block brush-1 texels left along U', () => {
    expect(mirrorBlockAnchor({ x: 39, y: 52 }, 1)).toEqual({ x: 39, y: 52 });
    expect(mirrorBlockAnchor({ x: 39, y: 52 }, 2)).toEqual({ x: 38, y: 52 });
    expect(mirrorBlockAnchor({ x: 39, y: 52 }, 3)).toEqual({ x: 37, y: 52 });
  });
});
