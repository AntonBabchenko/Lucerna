// Full 64x64 skin UV atlas: every face of every part, base + overlay, for both
// model variants. Origins and face naming mirror skinview3d's setSkinUVs/setUVs
// (node_modules/skinview3d/libs/model.js) — that is the authority the 3D
// raycast UVs come from, so the 2D companion must agree with it texel-for-texel.

import type { Variant } from '$lib/accounts/skin-render';

export type Part = 'head' | 'body' | 'rightArm' | 'leftArm' | 'rightLeg' | 'leftLeg';
export type Layer = 'base' | 'overlay';
export type Face = 'top' | 'bottom' | 'left' | 'front' | 'right' | 'back';

export interface FaceRect {
  x: number;
  y: number;
  w: number;
  h: number;
  part: Part;
  layer: Layer;
  face: Face;
}

// The 6 faces of one box net at origin (ox,oy) with texel dims w x h x d.
// Net layout (setUVs): row 1 = [top][bottom] offset by d; row 2 = [left][front][right][back].
function box(part: Part, layer: Layer, ox: number, oy: number, w: number, h: number, d: number): FaceRect[] {
  const f = (x: number, y: number, fw: number, fh: number, face: Face): FaceRect => ({
    x,
    y,
    w: fw,
    h: fh,
    part,
    layer,
    face,
  });
  return [
    f(ox + d, oy, w, d, 'top'),
    f(ox + d + w, oy, w, d, 'bottom'),
    f(ox, oy + d, d, h, 'left'),
    f(ox + d, oy + d, w, h, 'front'),
    f(ox + d + w, oy + d, d, h, 'right'),
    f(ox + d + w + d, oy + d, w, h, 'back'),
  ];
}

// Origins match skinview3d's setSkinUVs calls (model.js lines 161-264).
function rectsFor(variant: Variant): FaceRect[] {
  const aw = variant === 'slim' ? 3 : 4; // arm width
  return [
    ...box('head', 'base', 0, 0, 8, 8, 8),
    ...box('head', 'overlay', 32, 0, 8, 8, 8),
    ...box('body', 'base', 16, 16, 8, 12, 4),
    ...box('body', 'overlay', 16, 32, 8, 12, 4),
    ...box('rightArm', 'base', 40, 16, aw, 12, 4),
    ...box('rightArm', 'overlay', 40, 32, aw, 12, 4),
    ...box('leftArm', 'base', 32, 48, aw, 12, 4),
    ...box('leftArm', 'overlay', 48, 48, aw, 12, 4),
    ...box('rightLeg', 'base', 0, 16, 4, 12, 4),
    ...box('rightLeg', 'overlay', 0, 32, 4, 12, 4),
    ...box('leftLeg', 'base', 16, 48, 4, 12, 4),
    ...box('leftLeg', 'overlay', 0, 48, 4, 12, 4),
  ];
}

const CACHE: Partial<Record<Variant, FaceRect[]>> = {};

export function allFaceRects(variant: Variant): FaceRect[] {
  CACHE[variant] ??= rectsFor(variant);
  return CACHE[variant];
}

export function faceRectAt(x: number, y: number, variant: Variant): FaceRect | undefined {
  return allFaceRects(variant).find(
    (r) => x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h,
  );
}

// Horizontal mirror within the containing face (v1 mirror-X). Full cross-limb
// symmetry is a future enhancement.
export function mirrorInFace(x: number, y: number, variant: Variant): { x: number; y: number } {
  const r = faceRectAt(x, y, variant);
  if (!r) return { x, y };
  return { x: r.x + (r.x + r.w - 1 - x), y };
}
