// A thin outline loop drawn on the 3D model surface marking the brush footprint
// while hovering. It lives in the viewer scene and is fed world-space corners by
// pickFootprint — it never touches the skin texture, so it can't corrupt a save.
import type { BufferAttribute, Vector3 } from 'three';
import { BufferGeometry, LineBasicMaterial, LineLoop, Vector3 as V3 } from 'three';

export function createBrushCursor(): LineLoop {
  const geometry = new BufferGeometry().setFromPoints([new V3(), new V3(), new V3(), new V3()]);
  const material = new LineBasicMaterial({
    color: 0xffffff,
    depthTest: false, // always visible on the surface
    transparent: true,
    opacity: 0.9,
  });
  const loop = new LineLoop(geometry, material);
  loop.renderOrder = 1000;
  return loop;
}

export function updateBrushCursor(loop: LineLoop, corners: Vector3[]): void {
  const pos = loop.geometry.getAttribute('position') as BufferAttribute;
  for (let i = 0; i < 4; i++) pos.setXYZ(i, corners[i].x, corners[i].y, corners[i].z);
  pos.needsUpdate = true;
}

export function disposeBrushCursor(loop: LineLoop): void {
  loop.parent?.remove(loop);
  loop.geometry.dispose();
  (loop.material as LineBasicMaterial).dispose();
}
