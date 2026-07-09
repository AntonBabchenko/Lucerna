// Pure geometry for cape previews. The cape front face occupies (1,1)-(11,17)
// of a 64x32 texture; HD capes are integer multiples, so we scale by texW/64.

export interface Rect {
  sx: number;
  sy: number;
  sw: number;
  sh: number;
}

/** Front-face crop rect for a cape texture, or null if it isn't 2:1. */
export function capeFrontRect(texW: number, texH: number): Rect | null {
  if (texW <= 0 || texH <= 0 || texW !== texH * 2) return null;
  const s = texW / 64;
  return { sx: 1 * s, sy: 1 * s, sw: 10 * s, sh: 16 * s };
}

/**
 * Draw a cape's front face onto a canvas at integer scale, pixelated. `img`
 * must be fully loaded. No-op if the texture isn't a valid cape.
 */
export function drawCapeFront(canvas: HTMLCanvasElement, img: HTMLImageElement, scale = 6): void {
  const rect = capeFrontRect(img.naturalWidth, img.naturalHeight);
  if (!rect) return;
  canvas.width = 10 * scale;
  canvas.height = 16 * scale;
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  ctx.imageSmoothingEnabled = false;
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(img, rect.sx, rect.sy, rect.sw, rect.sh, 0, 0, canvas.width, canvas.height);
}
