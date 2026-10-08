import { describe, expect, it } from 'vitest';
import {
  BACKDROP_KINDS,
  type Backdrop,
  renderBackdrop,
  renderBackdropRows,
} from '$lib/accounts/skin-editor/backdrops';

// Small renders keep the suite fast; the scene is the same at any size, only coarser.
const W = 256;
const H = 128;
const renders = new Map(BACKDROP_KINDS.map((k) => [k, renderBackdrop(k, W, H)] as const));
const get = (k: (typeof BACKDROP_KINDS)[number]) => renders.get(k) as Backdrop;

/** Mean colour of a block of rows (all columns). */
function meanRows(b: Backdrop, y0: number, y1: number): [number, number, number] {
  const s = [0, 0, 0];
  for (let y = y0; y < y1; y += 1) {
    for (let x = 0; x < b.width; x += 1) {
      const i = (y * b.width + x) * 4;
      s[0] += b.data[i];
      s[1] += b.data[i + 1];
      s[2] += b.data[i + 2];
    }
  }
  const n = (y1 - y0) * b.width;
  return [s[0] / n, s[1] / n, s[2] / n];
}

/** Share of pixels in a block of rows that satisfy `test`. */
function share(
  b: Backdrop,
  y0: number,
  y1: number,
  test: (r: number, g: number, bl: number) => boolean,
) {
  let hit = 0;
  for (let y = y0; y < y1; y += 1) {
    for (let x = 0; x < b.width; x += 1) {
      const i = (y * b.width + x) * 4;
      if (test(b.data[i], b.data[i + 1], b.data[i + 2])) hit += 1;
    }
  }
  return hit / ((y1 - y0) * b.width);
}

describe('backdrops — every kind', () => {
  it.each(BACKDROP_KINDS)('%s is a fully opaque equirectangular image of the asked size', (k) => {
    const b = get(k);
    expect([b.width, b.height, b.data.length]).toEqual([W, H, W * H * 4]);
    for (let i = 3; i < b.data.length; i += 4) {
      if (b.data[i] !== 255) throw new Error(`transparent pixel at ${i >> 2}`);
    }
  });

  it.each(BACKDROP_KINDS)('%s is deterministic', (k) => {
    expect(renderBackdrop(k, W, H).data).toEqual(get(k).data);
  });

  // The editor renders in row slices so the window never freezes; the slices must add up to
  // exactly the whole picture.
  it.each(BACKDROP_KINDS)('%s rendered in row slices equals the whole render', (k) => {
    const data = new Uint8ClampedArray(W * H * 4);
    for (let y = 0; y < H; y += 24) renderBackdropRows(k, W, H, y, Math.min(H, y + 24), data);
    expect(data).toEqual(get(k).data);
  });
});

describe('backdrops — each place looks like itself', () => {
  it('plains: blue sky above, green grass below', () => {
    const sky = meanRows(get('plains'), 8, 40);
    const ground = meanRows(get('plains'), 96, 124);
    expect(sky[2]).toBeGreaterThan(sky[0] + 40);
    expect(ground[1]).toBeGreaterThan(ground[0] + 30);
    expect(ground[1]).toBeGreaterThan(ground[2] + 30);
  });

  // Underfoot is solid netherrack on purpose; the lava pools start a few blocks out, so they
  // sit in the band of floor just below the horizon (rows 64..72 of 128).
  it('nether: a dark red roof above, lava pools in the floor beyond the feet', () => {
    const roof = meanRows(get('nether'), 4, 30);
    expect(roof[0]).toBeGreaterThan(roof[1] * 1.6);
    expect(roof[0]).toBeLessThan(170);
    const isLava = (r: number, g: number, b: number) => r > 200 && g > 80 && b < 90;
    expect(share(get('nether'), 64, 72, isLava)).toBeGreaterThan(0.03);
    expect(share(get('nether'), 110, 128, isLava)).toBe(0);
  });

  it('stronghold: a grey brick wall with darker mortar around the eye line', () => {
    const wall = get('stronghold');
    const grey = share(wall, 56, 72, (r, g, b) => Math.abs(r - g) < 40 && Math.abs(g - b) < 40);
    expect(grey).toBeGreaterThan(0.8);
    const mortar = share(wall, 56, 72, (r, g, b) => r + g + b < 300);
    expect(mortar).toBeGreaterThan(0.05);
  });
});
