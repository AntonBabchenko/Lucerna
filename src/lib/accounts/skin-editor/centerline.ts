// Translucent guide plane at x=0 marking the left/right symmetry axis in the 3D
// skin viewer. Added while the mirror tool is on, removed and disposed otherwise.
// three.js is a direct dependency (see paint3d.ts); importing runtime classes is fine.
import { DoubleSide, Mesh, MeshBasicMaterial, PlaneGeometry } from 'three';

// The player model spans ~32 texels tall and ~8 deep; 40 covers it with margin.
const GUIDE_SIZE = 40;

export function createCenterlineGuide(): Mesh {
  const geometry = new PlaneGeometry(GUIDE_SIZE, GUIDE_SIZE);
  const material = new MeshBasicMaterial({
    color: 0x60a5fa,
    transparent: true,
    opacity: 0.16,
    side: DoubleSide,
    depthWrite: false,
  });
  const mesh = new Mesh(geometry, material);
  mesh.rotation.y = Math.PI / 2; // normal along x -> plane lies in the y-z plane at x=0
  mesh.renderOrder = 999;
  return mesh;
}

export function disposeGuide(mesh: Mesh): void {
  mesh.parent?.remove(mesh);
  mesh.geometry.dispose();
  (mesh.material as MeshBasicMaterial).dispose();
}
