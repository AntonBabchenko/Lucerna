// Pointer -> NDC -> uv -> texel resolution for painting on the 3D model.
// The raycast is restricted to the ACTIVE layer's visible meshes so the
// overlay (a slightly larger box enclosing the base) never steals a hit
// meant for the base layer — see the skin-editor spec, risk #1.

import type { Object3D, PerspectiveCamera } from 'three';
import { Raycaster, Vector2 } from 'three';
import { SKIN_SIZE } from './buffer';

export interface Ndc {
  x: number;
  y: number;
}

export function ndcFromPointer(clientX: number, clientY: number, rect: DOMRect): Ndc {
  return {
    x: ((clientX - rect.left) / rect.width) * 2 - 1,
    y: -((clientY - rect.top) / rect.height) * 2 + 1,
  };
}

const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));

// V flip: skinview3d's setUVs writes v as 1 - y/height, so texel y recovers as
// floor((1-v)*size). uv.x can be exactly 1.0 at a face edge -> clamp to size-1.
export function uvToTexel(
  uv: { x: number; y: number },
  size = SKIN_SIZE,
): { x: number; y: number } {
  return {
    x: clamp(Math.floor(uv.x * size), 0, size - 1),
    y: clamp(Math.floor((1 - uv.y) * size), 0, size - 1),
  };
}

const raycaster = new Raycaster();
const ndcVec = new Vector2();

/**
 * Raycast the supplied (active-layer, visible) meshes and return the hit skin
 * texel, or null on a miss. `meshes` must be pre-filtered to the layer being
 * painted; passing the whole player object would let the overlay occlude the base.
 */
export function pickTexel(
  camera: PerspectiveCamera,
  meshes: Object3D[],
  clientX: number,
  clientY: number,
  rect: DOMRect,
): { x: number; y: number } | null {
  const ndc = ndcFromPointer(clientX, clientY, rect);
  ndcVec.set(ndc.x, ndc.y);
  raycaster.setFromCamera(ndcVec, camera);
  const hit = raycaster.intersectObjects(meshes, true).find((h) => h.uv);
  if (!hit?.uv) return null;
  return uvToTexel(hit.uv);
}
