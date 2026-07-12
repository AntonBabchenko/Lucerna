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
function box(
  part: Part,
  layer: Layer,
  ox: number,
  oy: number,
  w: number,
  h: number,
  d: number,
): FaceRect[] {
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
  return allFaceRects(variant).find((r) => x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h);
}

const PART_MIRROR: Record<Part, Part> = {
  head: 'head',
  body: 'body',
  rightArm: 'leftArm',
  leftArm: 'rightArm',
  rightLeg: 'leftLeg',
  leftLeg: 'rightLeg',
};

const FACE_MIRROR: Record<Face, Face> = {
  top: 'top',
  bottom: 'bottom',
  left: 'right',
  front: 'front',
  right: 'left',
  back: 'back',
};

const RECT_INDEX: Partial<Record<Variant, Map<string, FaceRect>>> = {};

function rectFor(variant: Variant, part: Part, layer: Layer, face: Face): FaceRect | undefined {
  let idx = RECT_INDEX[variant];
  if (!idx) {
    idx = new Map();
    for (const r of allFaceRects(variant)) idx.set(`${r.part}:${r.layer}:${r.face}`, r);
    RECT_INDEX[variant] = idx;
  }
  return idx.get(`${part}:${layer}:${face}`);
}

// Sagittal (x=0) body mirror: cross-limb part swap + left/right face swap, flip
// U on every face, never flip V, same layer. Returns null when (x,y) is outside
// every face. Counterpart faces share dimensions within a variant, so this is an
// involution: mirrorTexel(mirrorTexel(p)) === p.
export function mirrorTexel(
  x: number,
  y: number,
  variant: Variant,
): { x: number; y: number } | null {
  const r = faceRectAt(x, y, variant);
  if (!r) return null;
  const dest = rectFor(variant, PART_MIRROR[r.part], r.layer, FACE_MIRROR[r.face]);
  if (!dest) return null;
  const lx = x - r.x;
  const ly = y - r.y;
  return { x: dest.x + (dest.w - 1 - lx), y: dest.y + ly }; // flip U, keep V
}

// A brush×brush block is centred on the cursor by shifting its top-left up/left
// by this offset (0 for size 1-2, 1 for size 3, …).
export function brushOffset(brush: number): number {
  return Math.floor((brush - 1) / 2);
}

// Top-left anchor for the mirror of a centred brush block. The mirror flips U, so
// the mirrored square's top-left sits (brush-1-offset) texels left and `offset`
// up of the mirrored centre. Fill ignores brush and uses the mirrored seed
// directly (do not call this for fill).
export function mirrorBlockAnchor(
  m: { x: number; y: number },
  brush: number,
): { x: number; y: number } {
  const off = brushOffset(brush);
  return { x: m.x - (brush - 1 - off), y: m.y - off };
}
