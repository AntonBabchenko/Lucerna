// A translucent highlight of the brush footprint on the 3D model surface, drawn
// as one quad per targeted texel. Because each texel is placed on its own face,
// the highlight wraps around the model's edges. It lives in the viewer scene and
// never touches the skin texture, so it can't corrupt a save.
import type { BufferAttribute, Vector3 } from 'three';
import { BufferGeometry, DoubleSide, Float32BufferAttribute, Mesh, MeshBasicMaterial } from 'three';

// Brush max is 5 → 25 texels; keep headroom so the buffer never reallocates.
const MAX_QUADS = 36;
const VERTS_PER_QUAD = 6; // two triangles

export function createBrushCursor(): Mesh {
  const geometry = new BufferGeometry();
  geometry.setAttribute(
    'position',
    new Float32BufferAttribute(new Float32Array(MAX_QUADS * VERTS_PER_QUAD * 3), 3),
  );
  geometry.setDrawRange(0, 0);
  const material = new MeshBasicMaterial({
    color: 0xffffff,
    transparent: true,
    opacity: 0.35,
    depthTest: false, // always visible on the surface
    side: DoubleSide,
  });
  const mesh = new Mesh(geometry, material);
  mesh.renderOrder = 1000;
  mesh.frustumCulled = false;
  return mesh;
}

/** Each quad is 4 world corners [tl, tr, br, bl]; drawn as two triangles. */
export function updateBrushCursor(mesh: Mesh, quads: Vector3[][]): void {
  const pos = mesh.geometry.getAttribute('position') as BufferAttribute;
  const arr = pos.array as Float32Array;
  const n = Math.min(quads.length, MAX_QUADS);
  let i = 0;
  for (let q = 0; q < n; q++) {
    const c = quads[q];
    for (const idx of [0, 1, 2, 0, 2, 3]) {
      arr[i++] = c[idx].x;
      arr[i++] = c[idx].y;
      arr[i++] = c[idx].z;
    }
  }
  pos.needsUpdate = true;
  mesh.geometry.setDrawRange(0, n * VERTS_PER_QUAD);
}

export function disposeBrushCursor(mesh: Mesh): void {
  mesh.parent?.remove(mesh);
  mesh.geometry.dispose();
  (mesh.material as MeshBasicMaterial).dispose();
}
