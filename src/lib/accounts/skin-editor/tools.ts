// Pure drawing operations on the 64x64 skin buffer. All tools take texel
// coordinates; the UI layer resolves pointer events to texels first.

import type { FaceRect } from './atlas';
import { getTexel, type Rgba, SKIN_SIZE, setTexel } from './buffer';

export function pencil(
  data: Uint8ClampedArray,
  x: number,
  y: number,
  colour: Rgba,
  brush = 1,
): void {
  for (let dy = 0; dy < brush; dy++) {
    for (let dx = 0; dx < brush; dx++) setTexel(data, x + dx, y + dy, colour);
  }
}

export function eraser(data: Uint8ClampedArray, x: number, y: number, brush = 1): void {
  for (let dy = 0; dy < brush; dy++) {
    for (let dx = 0; dx < brush; dx++) setTexel(data, x + dx, y + dy, [0, 0, 0, 0]);
  }
}

export function pickColour(data: Uint8ClampedArray, x: number, y: number): Rgba {
  return getTexel(data, x, y);
}

function sameColour(a: Rgba, b: Rgba): boolean {
  return a[0] === b[0] && a[1] === b[1] && a[2] === b[2] && a[3] === b[3];
}

// 4-way flood fill of the contiguous same-colour region, clamped to `bounds`
// (the hit face) so a fill never bleeds across a cuboid seam in the atlas.
export function fill(
  data: Uint8ClampedArray,
  x: number,
  y: number,
  colour: Rgba,
  bounds: FaceRect,
): void {
  const target = getTexel(data, x, y);
  if (sameColour(target, colour)) return;
  const inBounds = (px: number, py: number): boolean =>
    px >= bounds.x && px < bounds.x + bounds.w && py >= bounds.y && py < bounds.y + bounds.h;
  const stack: Array<[number, number]> = [[x, y]];
  while (stack.length > 0) {
    const [px, py] = stack.pop() as [number, number];
    if (!inBounds(px, py)) continue;
    if (!sameColour(getTexel(data, px, py), target)) continue;
    setTexel(data, px, py, colour);
    stack.push([px + 1, py], [px - 1, py], [px, py + 1], [px, py - 1]);
  }
}

// --- colour maths (RGB <-> HSL), used by dodge/burn ---

function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  const rn = r / 255;
  const gn = g / 255;
  const bn = b / 255;
  const max = Math.max(rn, gn, bn);
  const min = Math.min(rn, gn, bn);
  const l = (max + min) / 2;
  const d = max - min;
  const s = d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1));
  let h = 0;
  if (d !== 0) {
    if (max === rn) h = ((gn - bn) / d) % 6;
    else if (max === gn) h = (bn - rn) / d + 2;
    else h = (rn - gn) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  return [h, s, l];
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  let r = 0;
  let g = 0;
  let b = 0;
  if (h < 60) [r, g, b] = [c, x, 0];
  else if (h < 120) [r, g, b] = [x, c, 0];
  else if (h < 180) [r, g, b] = [0, c, x];
  else if (h < 240) [r, g, b] = [0, x, c];
  else if (h < 300) [r, g, b] = [x, 0, c];
  else [r, g, b] = [c, 0, x];
  return [Math.round((r + m) * 255), Math.round((g + m) * 255), Math.round((b + m) * 255)];
}

const clamp01 = (v: number): number => Math.min(1, Math.max(0, v));

/** Shift HSL lightness by deltaL (dodge > 0, burn < 0); hue preserved. */
export function dodgeBurn(
  data: Uint8ClampedArray,
  x: number,
  y: number,
  deltaL: number,
  brush = 1,
): void {
  for (let dy = 0; dy < brush; dy++) {
    for (let dx = 0; dx < brush; dx++) {
      const c = getTexel(data, x + dx, y + dy);
      if (c[3] === 0) continue;
      const [h, s, l] = rgbToHsl(c[0], c[1], c[2]);
      const [r, g, b] = hslToRgb(h, s, clamp01(l + deltaL));
      setTexel(data, x + dx, y + dy, [r, g, b, c[3]]);
    }
  }
}

/** Jitter brightness by up to ±amount per texel. rng() in [0,1); injectable for tests. */
export function noise(
  data: Uint8ClampedArray,
  x: number,
  y: number,
  amount: number,
  brush = 1,
  rng: () => number = Math.random,
): void {
  for (let dy = 0; dy < brush; dy++) {
    for (let dx = 0; dx < brush; dx++) {
      const c = getTexel(data, x + dx, y + dy);
      if (c[3] === 0) continue;
      const delta = Math.round((rng() * 2 - 1) * amount);
      setTexel(data, x + dx, y + dy, [c[0] + delta, c[1] + delta, c[2] + delta, c[3]]);
    }
  }
}

// Clear every texel not covered by any face rect (the gaps outside the UV
// layout, which never render on the model). Returns true if anything changed, so
// callers can skip a no-op history entry. `rects` is the layout for the variant.
export function clearOutsideAtlas(data: Uint8ClampedArray, rects: FaceRect[]): boolean {
  const used = new Uint8Array(SKIN_SIZE * SKIN_SIZE);
  for (const r of rects) {
    for (let y = r.y; y < r.y + r.h; y++) {
      for (let x = r.x; x < r.x + r.w; x++) used[y * SKIN_SIZE + x] = 1;
    }
  }
  let changed = false;
  for (let y = 0; y < SKIN_SIZE; y++) {
    for (let x = 0; x < SKIN_SIZE; x++) {
      if (used[y * SKIN_SIZE + x]) continue;
      const i = (y * SKIN_SIZE + x) * 4;
      if (data[i] || data[i + 1] || data[i + 2] || data[i + 3]) {
        setTexel(data, x, y, [0, 0, 0, 0]);
        changed = true;
      }
    }
  }
  return changed;
}
