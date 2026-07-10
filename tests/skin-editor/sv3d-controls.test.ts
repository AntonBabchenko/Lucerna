import { MOUSE } from 'three';
import { describe, expect, it, vi } from 'vitest';
import type { SkinViewer } from 'skinview3d';
import { applyViewerControls } from '$lib/accounts/sv3d-controls';

type Listener = (e: unknown) => void;

function fakeViewer(): SkinViewer {
  return {
    controls: {
      enablePan: false,
      mouseButtons: {} as { LEFT?: number; MIDDLE?: number; RIGHT?: number },
    },
  } as unknown as SkinViewer;
}

function fakeCanvas() {
  const listeners = new Map<string, Listener>();
  const canvas = {
    addEventListener: vi.fn((type: string, fn: Listener) => listeners.set(type, fn)),
    removeEventListener: vi.fn((type: string) => listeners.delete(type)),
  } as unknown as HTMLCanvasElement;
  return { canvas, listeners };
}

describe('applyViewerControls', () => {
  it('maps LEFT+RIGHT to rotate, MIDDLE to pan, and enables pan', () => {
    const viewer = fakeViewer();
    const { canvas } = fakeCanvas();
    applyViewerControls(viewer, canvas);
    expect(viewer.controls.enablePan).toBe(true);
    expect(viewer.controls.mouseButtons).toEqual({
      LEFT: MOUSE.ROTATE,
      MIDDLE: MOUSE.PAN,
      RIGHT: MOUSE.ROTATE,
    });
  });

  it('suppresses the context menu', () => {
    const { canvas, listeners } = fakeCanvas();
    applyViewerControls(fakeViewer(), canvas);
    const e = { preventDefault: vi.fn() };
    listeners.get('contextmenu')?.(e);
    expect(e.preventDefault).toHaveBeenCalledOnce();
  });

  it('suppresses middle-click autoscroll only (not left/right)', () => {
    const { canvas, listeners } = fakeCanvas();
    applyViewerControls(fakeViewer(), canvas);
    const mousedown = listeners.get('mousedown');
    const middle = { button: 1, preventDefault: vi.fn() };
    const left = { button: 0, preventDefault: vi.fn() };
    mousedown?.(middle);
    mousedown?.(left);
    expect(middle.preventDefault).toHaveBeenCalledOnce();
    expect(left.preventDefault).not.toHaveBeenCalled();
  });

  it('cleanup removes both listeners', () => {
    const { canvas } = fakeCanvas();
    const cleanup = applyViewerControls(fakeViewer(), canvas);
    cleanup();
    expect(canvas.removeEventListener).toHaveBeenCalledWith('contextmenu', expect.any(Function));
    expect(canvas.removeEventListener).toHaveBeenCalledWith('mousedown', expect.any(Function));
  });
});
