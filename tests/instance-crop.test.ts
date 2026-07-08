import { describe, expect, it } from 'vitest';
import { computeCropRect } from '$lib/instances/crop';

describe('computeCropRect', () => {
  it('returns the whole image for a fitted square', () => {
    // 512x512 fitted into a 256 frame -> scale 0.5, centered (offset 0).
    expect(
      computeCropRect({ imgW: 512, imgH: 512, scale: 0.5, offsetX: 0, offsetY: 0, frame: 256 }),
    ).toEqual({ sx: 0, sy: 0, sSize: 512 });
  });

  it('returns the centered half when zoomed 2x', () => {
    // scale 1.0 (2x of min 0.5); centered offset = (256-512)/2 = -128.
    expect(
      computeCropRect({ imgW: 512, imgH: 512, scale: 1, offsetX: -128, offsetY: -128, frame: 256 }),
    ).toEqual({ sx: 128, sy: 128, sSize: 256 });
  });

  it('clamps the crop inside the image when panned past the edge', () => {
    const r = computeCropRect({
      imgW: 512,
      imgH: 512,
      scale: 1,
      offsetX: -1000,
      offsetY: 0,
      frame: 256,
    });
    expect(r.sSize).toBe(256);
    expect(r.sx).toBe(256); // imgW - sSize
    expect(r.sy).toBe(0);
  });

  it('uses the short side for a landscape image', () => {
    // 800x400, scale 0.64 (256/400), offset 0 -> sSize = min(400, 800, 400) = 400.
    const r = computeCropRect({
      imgW: 800,
      imgH: 400,
      scale: 0.64,
      offsetX: 0,
      offsetY: 0,
      frame: 256,
    });
    expect(r.sSize).toBe(400);
  });
});
