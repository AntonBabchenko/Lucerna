// Pure pixel helpers for the 64x64 Minecraft skin atlas. The editor treats the
// skin as one flat RGBA buffer (base and overlay are distinct REGIONS of the
// same texture, not stacked layers) — see the skin-editor spec.

export const SKIN_SIZE = 64;

export type Rgba = [r: number, g: number, b: number, a: number];

const inRange = (x: number, y: number): boolean =>
  x >= 0 && x < SKIN_SIZE && y >= 0 && y < SKIN_SIZE;

export function createBlankSkin(): Uint8ClampedArray {
  return new Uint8ClampedArray(SKIN_SIZE * SKIN_SIZE * 4);
}

export function getTexel(data: Uint8ClampedArray, x: number, y: number): Rgba {
  if (!inRange(x, y)) return [0, 0, 0, 0];
  const i = (y * SKIN_SIZE + x) * 4;
  return [data[i], data[i + 1], data[i + 2], data[i + 3]];
}

export function setTexel(data: Uint8ClampedArray, x: number, y: number, c: Rgba): void {
  if (!inRange(x, y)) return;
  const i = (y * SKIN_SIZE + x) * 4;
  data[i] = c[0];
  data[i + 1] = c[1];
  data[i + 2] = c[2];
  data[i + 3] = c[3];
}

export function snapshot(data: Uint8ClampedArray): Uint8ClampedArray {
  return new Uint8ClampedArray(data);
}

export function restore(dst: Uint8ClampedArray, snap: Uint8ClampedArray): void {
  dst.set(snap);
}

export type SkinDimensions = 'ok' | 'legacy' | 'invalid';

/** 64x64 is the modern format; 64x32 is legacy (skinview3d upscales it). */
export function validateSkinDimensions(w: number, h: number): SkinDimensions {
  if (w === 64 && h === 64) return 'ok';
  if (w === 64 && h === 32) return 'legacy';
  return 'invalid';
}
