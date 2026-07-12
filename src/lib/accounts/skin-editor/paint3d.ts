// Pointer -> NDC -> uv -> texel resolution for painting on the 3D model.
// The raycast is restricted to the ACTIVE layer's visible meshes so the
// overlay (a slightly larger box enclosing the base) never steals a hit
// meant for the base layer — see the skin-editor spec, risk #1.

import type { BufferAttribute, Mesh, Object3D, PerspectiveCamera } from 'three';
import { Raycaster, Vector2, Vector3 } from 'three';
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

/**
 * Like pickTexel, but also returns the four world-space corners of the brush
 * footprint (brush×brush texels anchored at the hit texel) on the hit face, so
 * the UI can outline it on the model without touching the skin texture. Corners
 * are nudged along the face normal to avoid z-fighting. Null on a miss.
 */
export function pickFootprint(
  camera: PerspectiveCamera,
  meshes: Object3D[],
  clientX: number,
  clientY: number,
  rect: DOMRect,
  brush: number,
): { texel: { x: number; y: number }; corners: Vector3[] } | null {
  const ndc = ndcFromPointer(clientX, clientY, rect);
  ndcVec.set(ndc.x, ndc.y);
  raycaster.setFromCamera(ndcVec, camera);
  const hit = raycaster.intersectObjects(meshes, true).find((h) => h.uv && h.face);
  if (!hit?.uv || !hit.face) return null;
  const texel = uvToTexel(hit.uv);
  const mesh = hit.object as Mesh;
  const posAttr = mesh.geometry.getAttribute('position') as BufferAttribute;
  const uvAttr = mesh.geometry.getAttribute('uv') as BufferAttribute;
  const { a, b, c } = hit.face;
  const pa = new Vector3().fromBufferAttribute(posAttr, a);
  const e1 = new Vector3().fromBufferAttribute(posAttr, b).sub(pa);
  const e2 = new Vector3().fromBufferAttribute(posAttr, c).sub(pa);
  const ua = new Vector2().fromBufferAttribute(uvAttr, a);
  const ub = new Vector2().fromBufferAttribute(uvAttr, b);
  const uc = new Vector2().fromBufferAttribute(uvAttr, c);
  const d1x = ub.x - ua.x;
  const d1y = ub.y - ua.y;
  const d2x = uc.x - ua.x;
  const d2y = uc.y - ua.y;
  const det = d1x * d2y - d2x * d1y;
  if (Math.abs(det) < 1e-9) return null;
  const localAt = (u: number, v: number): Vector3 => {
    const du = u - ua.x;
    const dv = v - ua.y;
    const s = (du * d2y - dv * d2x) / det;
    const t = (dv * d1x - du * d1y) / det;
    return pa.clone().addScaledVector(e1, s).addScaledVector(e2, t);
  };
  // Centre the brush×brush footprint on the hit texel.
  // uvToTexel recovers texel y as floor((1 - v) * size) → v = 1 - y / size.
  const off = Math.floor((brush - 1) / 2);
  const ax = texel.x - off;
  const ay = texel.y - off;
  const u0 = ax / SKIN_SIZE;
  const u1 = (ax + brush) / SKIN_SIZE;
  const v0 = 1 - ay / SKIN_SIZE;
  const v1 = 1 - (ay + brush) / SKIN_SIZE;
  const cornersLocal = [localAt(u0, v0), localAt(u1, v0), localAt(u1, v1), localAt(u0, v1)];
  mesh.updateWorldMatrix(true, false);
  const normal = hit.face.normal.clone().transformDirection(mesh.matrixWorld).normalize();
  const corners = cornersLocal.map((p) =>
    p.applyMatrix4(mesh.matrixWorld).addScaledVector(normal, 0.05),
  );
  return { texel, corners };
}
