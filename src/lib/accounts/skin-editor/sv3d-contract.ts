// Compile-time guard for the skinview3d public API the editor depends on (the
// "C1 contract" from the skin-editor spec). If any line here stops
// type-checking after a skinview3d upgrade, the contract broke — re-audit the
// editor's paint pipeline before shipping. skinview3d is pinned in package.json.

import type { SkinViewer } from 'skinview3d';
import type { Object3D, PerspectiveCamera, Texture, WebGLRenderer } from 'three';

export function assertSkinViewerContract(v: SkinViewer): void {
  const canvas: HTMLCanvasElement = v.skinCanvas;
  const camera: PerspectiveCamera = v.camera;
  const renderer: WebGLRenderer = v.renderer;
  const controlsEnabled: boolean = v.controls.enabled;
  const map: Texture | null = v.playerObject.skin.map;
  const inner: Object3D = v.playerObject.skin.head.innerLayer;
  const outer: Object3D = v.playerObject.skin.head.outerLayer;
  const modelType: 'default' | 'slim' = v.playerObject.skin.modelType;
  void canvas;
  void camera;
  void renderer;
  void controlsEnabled;
  void map;
  void inner;
  void outer;
  void modelType;
}
