// DOM glue between backdrops.ts (pure pixels) and skinview3d: draws a panorama into a canvas in
// row slices, yielding between them so the window never freezes (a full render takes about a
// second); keeps one canvas per place for the app's lifetime, so switching back is instant; and
// puts a background into a viewer.
import type { SkinViewer } from 'skinview3d';
import { NearestFilter, type Texture } from 'three';
import { type BackdropKind, renderBackdropRows } from '$lib/accounts/skin-editor/backdrops';
import { isPanorama, type ViewerBg } from '$lib/accounts/skin-editor/viewer-bg';

const WIDTH = 2048;
const HEIGHT = 1024;
const SLICE = 32; // rows per slice: ~20–40 ms of work, then the window gets a turn

const drawn = new Map<BackdropKind, Promise<HTMLCanvasElement>>();

const nextTask = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

async function draw(kind: BackdropKind): Promise<HTMLCanvasElement> {
  const data = new Uint8ClampedArray(WIDTH * HEIGHT * 4);
  for (let y = 0; y < HEIGHT; y += SLICE) {
    renderBackdropRows(kind, WIDTH, HEIGHT, y, Math.min(HEIGHT, y + SLICE), data);
    await nextTask();
  }
  const canvas = document.createElement('canvas');
  canvas.width = WIDTH;
  canvas.height = HEIGHT;
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('no 2D context for the panorama canvas');
  ctx.putImageData(new ImageData(data, WIDTH, HEIGHT), 0, 0);
  return canvas;
}

/** The panorama for a place, drawn once per app run. */
export function panoramaCanvas(kind: BackdropKind): Promise<HTMLCanvasElement> {
  const cached = drawn.get(kind);
  if (cached) return cached;
  const pending = draw(kind);
  drawn.set(kind, pending);
  // A failed draw is not remembered as the answer: the next pick draws again. The caller
  // still receives this rejection and reports it.
  pending.catch(() => drawn.delete(kind));
  return pending;
}

/**
 * Show `bg` in `viewer`. Neutral clears the 3D background, so the CSS gradient behind the
 * transparent canvas shows. A panorama is drawn first (or taken from the cache), and
 * `stillWanted` is asked once it is ready: a choice that changed meanwhile, or a viewer that was
 * disposed, does not get an outdated background.
 */
export async function applyViewerBackground(
  viewer: SkinViewer,
  bg: ViewerBg,
  stillWanted: () => boolean,
): Promise<void> {
  if (!isPanorama(bg)) {
    viewer.background = null;
    return;
  }
  const canvas = await panoramaCanvas(bg);
  if (!stillWanted() || viewer.disposed) return;
  viewer.loadPanorama(canvas);
  const texture = viewer.background;
  if (texture !== null && 'isTexture' in texture) {
    const t = texture as Texture;
    // Pixel art stays crisp. No colour-space tag: skinview3d turns three's colour management
    // off, so the canvas colours go to the screen as they are, like the skin's own texture.
    t.magFilter = NearestFilter;
    t.minFilter = NearestFilter;
    t.generateMipmaps = false;
    t.needsUpdate = true;
  }
}
