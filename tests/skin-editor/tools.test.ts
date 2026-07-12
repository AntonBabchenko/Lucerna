import { describe, expect, it } from 'vitest';
import { allFaceRects, type FaceRect, faceRectAt } from '$lib/accounts/skin-editor/atlas';
import { createBlankSkin, getTexel, setTexel } from '$lib/accounts/skin-editor/buffer';
import {
  clearOutsideAtlas,
  dodgeBurn,
  eraser,
  fill,
  noise,
  pencil,
  pickColour,
} from '$lib/accounts/skin-editor/tools';

function mustFaceRectAt(x: number, y: number): FaceRect {
  const rect = faceRectAt(x, y, 'classic');
  if (!rect) throw new Error(`no face rect at (${x}, ${y})`);
  return rect;
}

describe('tools', () => {
  it('pencil paints a 1px texel', () => {
    const d = createBlankSkin();
    pencil(d, 4, 4, [255, 0, 0, 255], 1);
    expect(getTexel(d, 4, 4)).toEqual([255, 0, 0, 255]);
    expect(getTexel(d, 5, 4)).toEqual([0, 0, 0, 0]);
  });

  it('pencil brush=2 paints a 2x2 block', () => {
    const d = createBlankSkin();
    pencil(d, 4, 4, [1, 2, 3, 255], 2);
    expect(getTexel(d, 4, 4)[3]).toBe(255);
    expect(getTexel(d, 5, 5)[3]).toBe(255);
    expect(getTexel(d, 6, 6)[3]).toBe(0);
  });

  it('eraser clears to transparent', () => {
    const d = createBlankSkin();
    setTexel(d, 2, 2, [9, 9, 9, 255]);
    eraser(d, 2, 2, 1);
    expect(getTexel(d, 2, 2)).toEqual([0, 0, 0, 0]);
  });

  it('eyedropper reads a texel', () => {
    const d = createBlankSkin();
    setTexel(d, 7, 7, [12, 34, 56, 255]);
    expect(pickColour(d, 7, 7)).toEqual([12, 34, 56, 255]);
  });

  it('fill floods a same-colour region bounded to the face', () => {
    const d = createBlankSkin();
    // paint head-front (x8..15,y8..15) all opaque white
    for (let y = 8; y < 16; y++)
      for (let x = 8; x < 16; x++) setTexel(d, x, y, [255, 255, 255, 255]);
    fill(d, 10, 10, [0, 0, 255, 255], mustFaceRectAt(10, 10));
    expect(getTexel(d, 8, 8)).toEqual([0, 0, 255, 255]);
    expect(getTexel(d, 15, 15)).toEqual([0, 0, 255, 255]);
    // a texel just outside the face rect is untouched
    expect(getTexel(d, 16, 8)).toEqual([0, 0, 0, 0]);
  });

  it('fill does not cross into a differently-coloured texel inside the face', () => {
    const d = createBlankSkin();
    for (let y = 8; y < 16; y++)
      for (let x = 8; x < 16; x++) setTexel(d, x, y, [255, 255, 255, 255]);
    setTexel(d, 12, 10, [0, 0, 0, 255]); // a black divider pixel
    fill(d, 9, 9, [255, 0, 0, 255], mustFaceRectAt(9, 9));
    expect(getTexel(d, 9, 9)).toEqual([255, 0, 0, 255]);
    expect(getTexel(d, 12, 10)).toEqual([0, 0, 0, 255]); // divider untouched
  });

  it('fill into the same colour is a no-op (no infinite loop)', () => {
    const d = createBlankSkin();
    setTexel(d, 10, 10, [5, 5, 5, 255]);
    fill(d, 10, 10, [5, 5, 5, 255], mustFaceRectAt(10, 10));
    expect(getTexel(d, 10, 10)).toEqual([5, 5, 5, 255]);
  });

  it('dodge brightens, burn darkens', () => {
    const d = createBlankSkin();
    setTexel(d, 1, 1, [100, 100, 100, 255]);
    dodgeBurn(d, 1, 1, +0.1, 1);
    expect(getTexel(d, 1, 1)[0]).toBeGreaterThan(100);
    setTexel(d, 2, 2, [100, 100, 100, 255]);
    dodgeBurn(d, 2, 2, -0.1, 1);
    expect(getTexel(d, 2, 2)[0]).toBeLessThan(100);
  });

  it('dodge/burn preserves hue and ignores fully transparent texels', () => {
    const d = createBlankSkin();
    dodgeBurn(d, 3, 3, +0.2, 1);
    expect(getTexel(d, 3, 3)).toEqual([0, 0, 0, 0]);
    // a saturated red stays red-dominant after dodging
    setTexel(d, 4, 4, [200, 40, 40, 255]);
    dodgeBurn(d, 4, 4, +0.1, 1);
    const c = getTexel(d, 4, 4);
    expect(c[0]).toBeGreaterThan(c[1]);
    expect(c[0]).toBeGreaterThan(c[2]);
  });

  it('noise jitters brightness deterministically with an injected rng', () => {
    const d = createBlankSkin();
    setTexel(d, 5, 5, [100, 100, 100, 255]);
    noise(d, 5, 5, 20, 1, () => 1); // rng=1 -> +max
    expect(getTexel(d, 5, 5)[0]).toBe(120);
  });

  it('noise ignores transparent texels', () => {
    const d = createBlankSkin();
    noise(d, 6, 6, 20, 1, () => 1);
    expect(getTexel(d, 6, 6)).toEqual([0, 0, 0, 0]);
  });

  it('clearOutsideAtlas wipes texels outside the UV layout but keeps used ones', () => {
    const d = createBlankSkin();
    setTexel(d, 10, 10, [1, 2, 3, 255]); // inside head-front
    setTexel(d, 63, 0, [9, 9, 9, 255]); // top-right corner gap: outside every rect
    expect(clearOutsideAtlas(d, allFaceRects('classic'))).toBe(true);
    expect(getTexel(d, 10, 10)).toEqual([1, 2, 3, 255]);
    expect(getTexel(d, 63, 0)).toEqual([0, 0, 0, 0]);
  });

  it('clearOutsideAtlas is a no-op when nothing is outside the layout', () => {
    const d = createBlankSkin();
    setTexel(d, 10, 10, [1, 2, 3, 255]);
    expect(clearOutsideAtlas(d, allFaceRects('classic'))).toBe(false);
    expect(getTexel(d, 10, 10)).toEqual([1, 2, 3, 255]);
  });
});
