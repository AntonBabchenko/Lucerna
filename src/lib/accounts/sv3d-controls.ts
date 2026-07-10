// Shared OrbitControls scheme for the launcher's skinview3d surfaces (the skin
// preview and the pixel editor). skinview3d's `viewer.controls` is a Three.js
// OrbitControls imported from three/examples, so the MOUSE enum from `three`
// maps cleanly onto controls.mouseButtons.
import type { SkinViewer } from 'skinview3d';
import { MOUSE } from 'three';

/**
 * Remap the viewer's OrbitControls to the launcher's 3D scheme:
 *   LEFT = rotate, RIGHT = rotate, MIDDLE = pan.
 * Enables pan (required for the MIDDLE mapping) and suppresses the two webview
 * defaults that fight these gestures:
 *   - the right-click context menu, so RMB-drag orbits without a popup;
 *   - middle-click autoscroll, so MMB-drag pans without the scroll puck.
 * Zoom is left untouched — each caller keeps its own `enableZoom` setting.
 *
 * Returns a cleanup that removes the DOM listeners; call it when the viewer is
 * disposed / the canvas unmounts.
 */
export function applyViewerControls(viewer: SkinViewer, canvas: HTMLCanvasElement): () => void {
  const controls = viewer.controls;
  controls.enablePan = true;
  controls.mouseButtons = { LEFT: MOUSE.ROTATE, MIDDLE: MOUSE.PAN, RIGHT: MOUSE.ROTATE };

  const onContextMenu = (e: Event): void => {
    e.preventDefault();
  };
  const onMouseDown = (e: MouseEvent): void => {
    if (e.button === 1) e.preventDefault(); // middle button → suppress autoscroll
  };
  canvas.addEventListener('contextmenu', onContextMenu);
  canvas.addEventListener('mousedown', onMouseDown);

  return () => {
    canvas.removeEventListener('contextmenu', onContextMenu);
    canvas.removeEventListener('mousedown', onMouseDown);
  };
}
