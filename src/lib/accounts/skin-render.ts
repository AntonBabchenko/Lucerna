// Pure skin geometry. Front-face UV map (texture-space rects) + a layout that
// places each part in a 2D front-body composite. A future skin editor reuses
// SKIN_UV to map canvas clicks back to texture pixels — this is the seam.

export type Variant = 'classic' | 'slim';

export interface SrcRect {
  sx: number;
  sy: number;
  sw: number;
  sh: number;
}
export interface DstRect {
  x: number;
  y: number;
  w: number;
  h: number;
}
export interface Part {
  name: string;
  src: SrcRect;
  dst: DstRect;
  overlay?: boolean;
}

const armW = (v: Variant) => (v === 'slim' ? 3 : 4);

/** Front-face source rects in a 64x64 skin texture (base layer + hat overlay). */
export const SKIN_UV = {
  head: { sx: 8, sy: 8, sw: 8, sh: 8 },
  hat: { sx: 40, sy: 8, sw: 8, sh: 8 },
  body: { sx: 20, sy: 20, sw: 8, sh: 12 },
  rightArm: { sx: 44, sy: 20, sw: 4, sh: 12 },
  leftArm: { sx: 36, sy: 52, sw: 4, sh: 12 },
  rightLeg: { sx: 4, sy: 20, sw: 4, sh: 12 },
  leftLeg: { sx: 20, sy: 52, sw: 4, sh: 12 },
} as const satisfies Record<string, SrcRect>;

/** Composite size (texel units) for a variant. Arms flank the 8-wide body. */
export function compositeSize(v: Variant): { w: number; h: number } {
  return { w: armW(v) + 8 + armW(v), h: 32 };
}

/**
 * Ordered draw list (base parts, then hat overlay). `texHeight === 32` (legacy
 * skins) has no left arm/leg in the texture, so the right ones are mirrored.
 */
export function frontLayout(v: Variant, texHeight = 64): Part[] {
  const a = armW(v);
  const legacy = texHeight === 32;
  const leftArmSrc = legacy ? SKIN_UV.rightArm : SKIN_UV.leftArm;
  const leftLegSrc = legacy ? SKIN_UV.rightLeg : SKIN_UV.leftLeg;
  const slimArm = (r: SrcRect): SrcRect => (v === 'slim' ? { ...r, sw: 3 } : r);
  return [
    { name: 'head', src: SKIN_UV.head, dst: { x: a, y: 0, w: 8, h: 8 } },
    { name: 'body', src: SKIN_UV.body, dst: { x: a, y: 8, w: 8, h: 12 } },
    { name: 'leftArm', src: slimArm(leftArmSrc), dst: { x: 0, y: 8, w: a, h: 12 } },
    { name: 'rightArm', src: slimArm(SKIN_UV.rightArm), dst: { x: a + 8, y: 8, w: a, h: 12 } },
    { name: 'rightLeg', src: SKIN_UV.rightLeg, dst: { x: a, y: 20, w: 4, h: 12 } },
    { name: 'leftLeg', src: leftLegSrc, dst: { x: a + 4, y: 20, w: 4, h: 12 } },
    { name: 'hat', src: SKIN_UV.hat, dst: { x: a, y: 0, w: 8, h: 8 }, overlay: true },
  ];
}

/** Render a skin PNG's front-body composite onto a canvas, pixelated. */
export function drawSkinFront(
  canvas: HTMLCanvasElement,
  img: HTMLImageElement,
  v: Variant,
  scale = 6,
): void {
  const { w, h } = compositeSize(v);
  canvas.width = w * scale;
  canvas.height = h * scale;
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  ctx.imageSmoothingEnabled = false;
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  for (const p of frontLayout(v, img.naturalHeight)) {
    ctx.drawImage(
      img,
      p.src.sx,
      p.src.sy,
      p.src.sw,
      p.src.sh,
      p.dst.x * scale,
      p.dst.y * scale,
      p.dst.w * scale,
      p.dst.h * scale,
    );
  }
}
