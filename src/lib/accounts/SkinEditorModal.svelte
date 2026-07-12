<script lang="ts">
  // Pixel skin editor: paints directly on the skinview3d 3D model, with a 2D
  // atlas companion for occluded faces and pixel-precise work. The single
  // source of truth is viewer.skinCanvas (the 64x64 texture the model renders
  // from); both views write it and flip skin.map.needsUpdate. All logic lives
  // in the pure skin-editor/* modules — this file only wires DOM and WebGL.
  // See the skin-editor spec (docs/superpowers/specs, local-only).
  import { onDestroy } from 'svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { Icon, type IconName } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { commands, type Account, type SkinVariant } from '$lib/ipc/bindings';
  import type { SkinViewer } from 'skinview3d';
  import type { LineLoop, Mesh, Object3D, Vector3 } from 'three';
  import { SKIN_SIZE, validateSkinDimensions, type Rgba } from '$lib/accounts/skin-editor/buffer';
  import {
    allFaceRects,
    faceRectAt,
    mirrorBlockAnchor,
    mirrorTexel,
  } from '$lib/accounts/skin-editor/atlas';
  import {
    dodgeBurn,
    eraser,
    fill,
    noise,
    pencil,
    pickColour,
  } from '$lib/accounts/skin-editor/tools';
  import { SkinHistory } from '$lib/accounts/skin-editor/history';
  import { footprintForTexel, pickFootprint, pickTexel } from '$lib/accounts/skin-editor/paint3d';
  import {
    createBrushCursor,
    disposeBrushCursor,
    updateBrushCursor,
  } from '$lib/accounts/skin-editor/brush-cursor';
  import { createCenterlineGuide, disposeGuide } from '$lib/accounts/skin-editor/centerline';
  import { POSE_NAMES, resolvePose, type PoseName } from '$lib/accounts/skin-editor/poses';
  import { assertSkinViewerContract } from '$lib/accounts/skin-editor/sv3d-contract';
  import { applyViewerControls } from '$lib/accounts/sv3d-controls';
  import {
    clampPanelWidth,
    companionCell,
    PANEL_KEY_STEP,
    PANEL_MAX_WIDTH,
    PANEL_MIN_WIDTH,
  } from '$lib/accounts/skin-editor/panel-resize';

  let {
    account,
    initialSkinB64,
    initialVariant,
    onClose,
    onApplied,
  }: {
    account: Account;
    initialSkinB64: string | null;
    initialVariant: SkinVariant;
    onClose: () => void;
    /** Called after a successful upload so the parent can refresh its preview. */
    onApplied?: (skinB64: string, variant: SkinVariant) => void;
  } = $props();

  type Tool = 'pencil' | 'eraser' | 'eyedropper' | 'fill' | 'dodge' | 'burn' | 'noise' | 'pan';
  let tool = $state<Tool>('pencil');
  let activeLayer = $state<'base' | 'overlay'>('base');
  let baseVisible = $state(true);
  let overlayVisible = $state(true);
  // svelte-ignore state_referenced_locally — the prop seeds the editable state
  // once; later parent changes must not clobber an in-progress edit.
  let variant = $state<SkinVariant>(initialVariant);
  let colour = $state<Rgba>([224, 224, 224, 255]);
  let brush = $state(1);
  let mirror = $state(false);
  let pose = $state<PoseName>('default');
  let showGrid = $state(true);
  let fullscreen = $state(false);
  let bg = $state<'dark' | 'light' | 'mid'>('dark');
  let busy = $state(false);
  let saveError = $state<string | null>(null);
  let applied = $state(false);
  let dirty = $state(false);
  let canUndo = $state(false);
  let canRedo = $state(false);
  let companionBoxWidth = $state(240); // measured companion box width (px), drives backing
  let resizeRowWidth = $state(880); // measured width of the 3D↔panel row
  // Let the panel grow to half the row (so in fullscreen the border reaches the
  // screen middle), but never below the fixed 640 default.
  let maxPanelWidth = $derived(Math.max(PANEL_MAX_WIDTH, Math.floor(resizeRowWidth / 2)));

  const isMicrosoft = $derived(account.kind === 'microsoft');
  const history = new SkinHistory(50);

  const variantToModel = (v: SkinVariant): 'default' | 'slim' =>
    v === 'slim' ? 'slim' : 'default';
  const PALETTE: Rgba[] = [
    [224, 224, 224, 255],
    [60, 60, 60, 255],
    [176, 125, 86, 255], // skin tone
    [122, 82, 52, 255], // darker skin tone
    [70, 49, 31, 255], // hair brown
    [46, 122, 158, 255], // shirt blue
    [58, 74, 138, 255], // trouser blue
    [163, 45, 45, 255], // red
    [59, 109, 17, 255], // green
    [239, 159, 39, 255], // amber
  ];

  let viewerCanvas: HTMLCanvasElement | null = null;
  let viewer: SkinViewer | null = null;
  let centerline: Mesh | null = null;
  let viewerBuilding = false;
  let disposeControls: (() => void) | null = null;
  let viewportBox: HTMLElement | null = null;
  let panelWidth = $state(300);
  let companion: HTMLCanvasElement | null = null;
  let companionBox: HTMLElement | null = null;
  let companionZoom = $state(1); // 2D companion magnification (1x–8x), wheel-controlled
  let companionPanning = false;
  let panStart = { x: 0, y: 0, scrollLeft: 0, scrollTop: 0 };
  let hoverTexel: { x: number; y: number } | null = null; // brush footprint preview anchor
  let brushCursor: LineLoop | null = null; // 3D footprint outline on the model surface
  let painting = false;
  let companionPainting = false;

  const rgbaToHex = (c: Rgba): string =>
    `#${c[0].toString(16).padStart(2, '0')}${c[1].toString(16).padStart(2, '0')}${c[2].toString(16).padStart(2, '0')}`;
  const hexToRgba = (hex: string): Rgba => [
    Number.parseInt(hex.slice(1, 3), 16),
    Number.parseInt(hex.slice(3, 5), 16),
    Number.parseInt(hex.slice(5, 7), 16),
    255,
  ];

  const POSE_LABEL: Record<PoseName, TranslationKey> = {
    default: 'skinEditor.poseDefault',
    tpose: 'skinEditor.poseTpose',
    walk: 'skinEditor.poseWalk',
    sit: 'skinEditor.poseSit',
  };

  const skinCtx = (): CanvasRenderingContext2D | null =>
    viewer?.skinCanvas.getContext('2d', { willReadFrequently: true }) ?? null;

  const readPixels = (): Uint8ClampedArray | null => {
    const ctx = skinCtx();
    return ctx ? ctx.getImageData(0, 0, SKIN_SIZE, SKIN_SIZE).data : null;
  };

  function pushPixels(pixels: Uint8ClampedArray): void {
    const ctx = skinCtx();
    if (!ctx || !viewer) return;
    ctx.putImageData(new ImageData(new Uint8ClampedArray(pixels), SKIN_SIZE, SKIN_SIZE), 0, 0);
    const tex = viewer.playerObject.skin.map;
    if (tex) tex.needsUpdate = true;
    renderCompanion();
  }

  // A "blank" skin is a grey mannequin (base faces filled, overlay transparent):
  // a fully transparent texture would render an invisible model.
  function blankSkinDataUrl(): string {
    const c = document.createElement('canvas');
    c.width = SKIN_SIZE;
    c.height = SKIN_SIZE;
    const ctx = c.getContext('2d');
    if (ctx) {
      for (const r of allFaceRects('classic')) {
        if (r.layer !== 'base') continue;
        ctx.fillStyle = r.part === 'head' ? '#a8a8a8' : r.part === 'body' ? '#909090' : '#9c9c9c';
        ctx.fillRect(r.x, r.y, r.w, r.h);
      }
    }
    return c.toDataURL('image/png');
  }

  function activeMeshes(): Object3D[] {
    if (!viewer) return [];
    const s = viewer.playerObject.skin;
    const key = activeLayer === 'base' ? 'innerLayer' : 'outerLayer';
    const out: Object3D[] = [];
    for (const part of [s.head, s.body, s.leftArm, s.rightArm, s.leftLeg, s.rightLeg]) {
      const mesh = part[key];
      if (mesh.visible) out.push(mesh);
    }
    return out;
  }

  async function buildViewer(): Promise<void> {
    if (viewer || viewerBuilding || !viewerCanvas) return;
    viewerBuilding = true;
    try {
      const skinview3d = await import('skinview3d');
      // The modal may have closed (canvas unmounted) during the async import.
      if (!viewerCanvas) return;
      const url =
        initialSkinB64 !== null ? `data:image/png;base64,${initialSkinB64}` : blankSkinDataUrl();
      viewer = new skinview3d.SkinViewer({
        canvas: viewerCanvas,
        width: 340,
        height: 400,
        skin: url,
        model: variantToModel(variant),
      });
      assertSkinViewerContract(viewer);
      viewer.controls.enableZoom = true;
      disposeControls = applyViewerControls(viewer, viewerCanvas);
      viewer.autoRotate = false;
      viewer.zoom = 0.8;
      // Static pose — a moving target is unpaintable. (No IdleAnimation here.)
      await viewer.loadSkin(url, { model: variantToModel(variant) });
      renderCompanion();
      fitViewport();
      syncCenterline();
      applyPose(pose);
    } finally {
      viewerBuilding = false;
    }
  }

  function syncCenterline(): void {
    if (!viewer) return;
    if (mirror && !centerline) {
      centerline = createCenterlineGuide();
      viewer.scene.add(centerline);
    } else if (!mirror && centerline) {
      disposeGuide(centerline);
      centerline = null;
    }
  }

  function mountViewer(node: HTMLCanvasElement) {
    viewerCanvas = node;
    void buildViewer();
    return {
      destroy() {
        if (centerline) {
          disposeGuide(centerline);
          centerline = null;
        }
        if (brushCursor) {
          disposeBrushCursor(brushCursor);
          brushCursor = null;
        }
        disposeControls?.();
        disposeControls = null;
        viewer?.dispose();
        viewer = null;
        viewerCanvas = null;
      },
    };
  }

  function fitViewport(): void {
    if (!viewer || !viewportBox) return;
    const w = Math.round(viewportBox.clientWidth);
    const h = Math.round(viewportBox.clientHeight);
    if (w > 0 && h > 0) viewer.setSize(w, h);
  }

  function observeViewport(node: HTMLElement) {
    viewportBox = node;
    const ro = new ResizeObserver(() => fitViewport());
    ro.observe(node);
    return {
      destroy() {
        ro.disconnect();
        viewportBox = null;
      },
    };
  }

  function observeResizeRow(node: HTMLElement) {
    const ro = new ResizeObserver(() => {
      const w = node.clientWidth;
      resizeRowWidth = w;
      // Re-clamp when the window shrinks or fullscreen is toggled off.
      panelWidth = clampPanelWidth(
        panelWidth,
        PANEL_MIN_WIDTH,
        Math.max(PANEL_MAX_WIDTH, Math.floor(w / 2)),
      );
    });
    ro.observe(node);
    return {
      destroy() {
        ro.disconnect();
      },
    };
  }

  function startPanelResize(e: PointerEvent): void {
    if (e.button !== 0) return;
    const startX = e.clientX;
    const startWidth = panelWidth;
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const onMove = (ev: PointerEvent): void => {
      // Panel is on the right: dragging left (smaller clientX) widens it.
      panelWidth = clampPanelWidth(
        startWidth - (ev.clientX - startX),
        PANEL_MIN_WIDTH,
        maxPanelWidth,
      );
    };
    const onUp = (ev: PointerEvent): void => {
      if (handle.hasPointerCapture(ev.pointerId)) handle.releasePointerCapture(ev.pointerId);
      handle.removeEventListener('pointermove', onMove);
      handle.removeEventListener('pointerup', onUp);
      handle.removeEventListener('pointercancel', onUp);
    };
    handle.addEventListener('pointermove', onMove);
    handle.addEventListener('pointerup', onUp);
    handle.addEventListener('pointercancel', onUp); // OS-cancelled pointer: still clean up
  }

  function onPanelResizeKey(e: KeyboardEvent): void {
    if (e.key === 'ArrowLeft')
      panelWidth = clampPanelWidth(panelWidth + PANEL_KEY_STEP, PANEL_MIN_WIDTH, maxPanelWidth);
    else if (e.key === 'ArrowRight')
      panelWidth = clampPanelWidth(panelWidth - PANEL_KEY_STEP, PANEL_MIN_WIDTH, maxPanelWidth);
    else return;
    e.preventDefault();
  }

  onDestroy(() => {
    disposeControls?.();
    disposeControls = null;
    viewer?.dispose();
    viewer = null;
  });

  // --- painting --------------------------------------------------------------

  function applyToolAt(x: number, y: number): void {
    const ctx = skinCtx();
    if (!ctx || !viewer) return;
    const img = ctx.getImageData(0, 0, SKIN_SIZE, SKIN_SIZE);
    const d = img.data;
    if (tool === 'eyedropper') {
      const picked = pickColour(d, x, y);
      if (picked[3] !== 0) colour = [picked[0], picked[1], picked[2], 255];
      return;
    }
    const paintOne = (px: number, py: number): void => {
      switch (tool) {
        case 'pencil':
          pencil(d, px, py, colour, brush);
          break;
        case 'eraser':
          eraser(d, px, py, brush);
          break;
        case 'dodge':
          dodgeBurn(d, px, py, +0.08, brush);
          break;
        case 'burn':
          dodgeBurn(d, px, py, -0.08, brush);
          break;
        case 'noise':
          noise(d, px, py, 24, brush);
          break;
        case 'fill': {
          const rect = faceRectAt(px, py, variant);
          if (rect) fill(d, px, py, colour, rect);
          break;
        }
        default:
          return;
      }
    };
    const off = Math.floor((brush - 1) / 2);
    if (tool === 'fill') paintOne(x, y);
    else paintOne(x - off, y - off);
    if (mirror) {
      const m = mirrorTexel(x, y, variant);
      if (m && (m.x !== x || m.y !== y)) {
        const anchor = tool === 'fill' ? m : mirrorBlockAnchor(m, brush);
        paintOne(anchor.x, anchor.y);
      }
    }
    ctx.putImageData(img, 0, 0);
    const tex = viewer.playerObject.skin.map;
    if (tex) tex.needsUpdate = true;
    dirty = true;
    renderCompanion();
  }

  function beginStroke(): void {
    const pixels = readPixels();
    if (pixels) {
      history.begin(pixels);
      syncHistoryFlags();
    }
  }

  function syncHistoryFlags(): void {
    canUndo = history.canUndo;
    canRedo = history.canRedo;
  }

  function paintFromViewerEvent(e: PointerEvent): void {
    if (!viewer || !viewerCanvas) return;
    const texel = pickTexel(
      viewer.camera,
      activeMeshes(),
      e.clientX,
      e.clientY,
      viewerCanvas.getBoundingClientRect(),
    );
    if (texel) applyToolAt(texel.x, texel.y);
  }

  function onViewerDown(e: PointerEvent): void {
    if (!viewer || busy) return;
    if (e.button !== 0) return; // right → orbit, middle → pan (OrbitControls handles it)
    if (tool === 'pan') return; // orbit stays enabled — drag rotates
    // Left paint: disable orbit for the stroke. OrbitControls' own canvas
    // listener may run first and arm STATE.ROTATE on this pointerdown, but it
    // applies no camera delta until pointermove — which early-returns while
    // controls.enabled is false — so the model can't rotate mid-paint.
    viewer.controls.enabled = false;
    painting = true;
    syncBrushCursor(null);
    viewerCanvas?.setPointerCapture(e.pointerId);
    beginStroke();
    paintFromViewerEvent(e);
  }

  function onViewerMove(e: PointerEvent): void {
    if (painting) {
      paintFromViewerEvent(e);
      return;
    }
    if (!viewer || !viewerCanvas) return;
    if (tool !== 'pencil' && tool !== 'eraser') {
      clearHover();
      return;
    }
    const hit = pickFootprint(
      viewer.camera,
      activeMeshes(),
      e.clientX,
      e.clientY,
      viewerCanvas.getBoundingClientRect(),
      brush,
    );
    setHoverTexel(hit?.texel ?? null);
    syncBrushCursor(hit?.corners ?? null);
  }

  function onViewerUp(e: PointerEvent): void {
    if (!painting) return;
    painting = false;
    viewerCanvas?.releasePointerCapture(e.pointerId);
    if (viewer) viewer.controls.enabled = true;
    syncHistoryFlags();
  }

  // --- 2D companion ----------------------------------------------------------

  function renderCompanion(): void {
    if (!companion || !viewer) return;
    const cell = companionCell(companionBoxWidth);
    const size = SKIN_SIZE * cell;
    if (companion.width !== size) {
      companion.width = size;
      companion.height = size;
    }
    const c = companion.getContext('2d');
    if (!c) return;
    c.imageSmoothingEnabled = false;
    c.clearRect(0, 0, size, size);
    c.drawImage(viewer.skinCanvas, 0, 0, SKIN_SIZE, SKIN_SIZE, 0, 0, size, size);
    if (showGrid && cell >= 4) {
      c.strokeStyle = 'rgba(128,128,128,0.15)';
      c.lineWidth = 1;
      c.beginPath();
      for (let i = 1; i < SKIN_SIZE; i++) {
        c.moveTo(i * cell + 0.5, 0);
        c.lineTo(i * cell + 0.5, size);
        c.moveTo(0, i * cell + 0.5);
        c.lineTo(size, i * cell + 0.5);
      }
      c.stroke();
    }
    if (mirror) {
      c.strokeStyle = 'rgba(96,165,250,0.6)';
      c.lineWidth = 1;
      c.beginPath();
      for (const r of allFaceRects(variant)) {
        if (r.part !== 'head' && r.part !== 'body') continue;
        if (r.face !== 'front' && r.face !== 'back' && r.face !== 'top' && r.face !== 'bottom') {
          continue;
        }
        const cx = (r.x + r.w / 2) * cell;
        c.moveTo(cx + 0.5, r.y * cell);
        c.lineTo(cx + 0.5, (r.y + r.h) * cell);
      }
      c.stroke();
    }
    if (hoverTexel && (tool === 'pencil' || tool === 'eraser')) {
      const off = Math.floor((brush - 1) / 2);
      const bx = (hoverTexel.x - off) * cell;
      const by = (hoverTexel.y - off) * cell;
      const bs = brush * cell;
      // Thin 1px outline in difference mode: always contrasts, never bulky.
      c.save();
      c.globalCompositeOperation = 'difference';
      c.strokeStyle = '#ffffff';
      c.lineWidth = 1;
      c.strokeRect(bx + 0.5, by + 0.5, bs - 1, bs - 1);
      c.restore();
    }
  }

  function setHoverTexel(t: { x: number; y: number } | null): void {
    if (t?.x === hoverTexel?.x && t?.y === hoverTexel?.y) return;
    hoverTexel = t;
    renderCompanion();
  }

  function syncBrushCursor(corners: Vector3[] | null): void {
    if (!viewer) return;
    if (corners) {
      if (!brushCursor) {
        brushCursor = createBrushCursor();
        viewer.scene.add(brushCursor);
      }
      updateBrushCursor(brushCursor, corners);
    } else if (brushCursor) {
      disposeBrushCursor(brushCursor);
      brushCursor = null;
    }
  }

  function clearHover(): void {
    setHoverTexel(null);
    syncBrushCursor(null);
  }

  // The visible layer mesh that a texel lives on, for outlining a 2D-hovered
  // brush on the 3D model. Null if the texel is off-atlas or its mesh is hidden.
  function meshForTexel(texel: { x: number; y: number }): Mesh | null {
    if (!viewer) return null;
    const r = faceRectAt(texel.x, texel.y, variant);
    if (!r) return null;
    const s = viewer.playerObject.skin;
    const parts = {
      head: s.head,
      body: s.body,
      rightArm: s.rightArm,
      leftArm: s.leftArm,
      rightLeg: s.rightLeg,
      leftLeg: s.leftLeg,
    };
    const mesh = (r.layer === 'base' ? parts[r.part].innerLayer : parts[r.part].outerLayer) as Mesh;
    return mesh.visible ? mesh : null;
  }

  function observeCompanionBox(node: HTMLElement) {
    companionBox = node;
    companionBoxWidth = node.clientWidth;
    const ro = new ResizeObserver(() => {
      companionBoxWidth = node.clientWidth;
      renderCompanion();
    });
    ro.observe(node);
    return {
      destroy() {
        ro.disconnect();
        companionBox = null;
      },
    };
  }

  function onCompanionWheel(e: WheelEvent): void {
    e.preventDefault();
    const factor = e.deltaY < 0 ? 1.25 : 0.8;
    companionZoom = Math.min(8, Math.max(1, companionZoom * factor));
  }

  function toggleMirror(): void {
    mirror = !mirror;
    renderCompanion();
    syncCenterline();
  }

  function companionTexel(e: PointerEvent): { x: number; y: number } | null {
    if (!companion) return null;
    const r = companion.getBoundingClientRect();
    const x = Math.floor(((e.clientX - r.left) / r.width) * SKIN_SIZE);
    const y = Math.floor(((e.clientY - r.top) / r.height) * SKIN_SIZE);
    if (x < 0 || x >= SKIN_SIZE || y < 0 || y >= SKIN_SIZE) return null;
    return { x, y };
  }

  function onCompanionDown(e: PointerEvent): void {
    if (busy) return;
    // Left button draws; right/middle (or the pan tool) pan the zoomed view.
    const wantsPan = e.button === 1 || e.button === 2 || (e.button === 0 && tool === 'pan');
    if (wantsPan) {
      if (!companionBox) return;
      e.preventDefault(); // suppress middle-button autoscroll / text selection
      companionPanning = true;
      companion?.setPointerCapture(e.pointerId);
      panStart = {
        x: e.clientX,
        y: e.clientY,
        scrollLeft: companionBox.scrollLeft,
        scrollTop: companionBox.scrollTop,
      };
      return;
    }
    if (e.button !== 0) return;
    companionPainting = true;
    companion?.setPointerCapture(e.pointerId);
    beginStroke();
    const texel = companionTexel(e);
    if (texel) applyToolAt(texel.x, texel.y);
  }

  function onCompanionMove(e: PointerEvent): void {
    if (companionPanning && companionBox) {
      companionBox.scrollLeft = panStart.scrollLeft - (e.clientX - panStart.x);
      companionBox.scrollTop = panStart.scrollTop - (e.clientY - panStart.y);
      return;
    }
    const texel = companionTexel(e);
    const brushHover = tool === 'pencil' || tool === 'eraser' ? texel : null;
    setHoverTexel(brushHover);
    const mesh = brushHover ? meshForTexel(brushHover) : null;
    syncBrushCursor(mesh && brushHover ? footprintForTexel(mesh, brushHover, brush) : null);
    if (!companionPainting) return;
    if (texel) applyToolAt(texel.x, texel.y);
  }

  function onCompanionUp(e: PointerEvent): void {
    if (companionPanning) {
      companionPanning = false;
      companion?.releasePointerCapture(e.pointerId);
      return;
    }
    if (!companionPainting) return;
    companionPainting = false;
    companion?.releasePointerCapture(e.pointerId);
    syncHistoryFlags();
  }

  // --- controls ---------------------------------------------------------------

  function undo(): void {
    const cur = readPixels();
    if (!cur) return;
    const prev = history.undo(cur);
    if (prev) {
      pushPixels(prev);
      dirty = true;
    }
    syncHistoryFlags();
  }

  function redo(): void {
    const cur = readPixels();
    if (!cur) return;
    const next = history.redo(cur);
    if (next) {
      pushPixels(next);
      dirty = true;
    }
    syncHistoryFlags();
  }

  function setVariant(v: SkinVariant): void {
    if (variant === v || !viewer) return;
    variant = v;
    viewer.playerObject.skin.modelType = variantToModel(v);
    applyPose(pose);
  }

  function applyPose(name: PoseName): void {
    if (!viewer) return;
    const rot = resolvePose(name);
    const s = viewer.playerObject.skin;
    s.head.rotation.set(rot.head.x, rot.head.y, rot.head.z);
    s.body.rotation.set(rot.body.x, rot.body.y, rot.body.z);
    s.rightArm.rotation.set(rot.rightArm.x, rot.rightArm.y, rot.rightArm.z);
    s.leftArm.rotation.set(rot.leftArm.x, rot.leftArm.y, rot.leftArm.z);
    s.rightLeg.rotation.set(rot.rightLeg.x, rot.rightLeg.y, rot.rightLeg.z);
    s.leftLeg.rotation.set(rot.leftLeg.x, rot.leftLeg.y, rot.leftLeg.z);
  }

  function setPose(name: PoseName): void {
    pose = name;
    applyPose(name);
  }

  function applyVisibility(): void {
    if (!viewer) return;
    const s = viewer.playerObject.skin;
    for (const part of [s.head, s.body, s.leftArm, s.rightArm, s.leftLeg, s.rightLeg]) {
      part.innerLayer.visible = baseVisible;
      part.outerLayer.visible = overlayVisible;
    }
  }

  function toggleBase(): void {
    baseVisible = !baseVisible;
    applyVisibility();
  }

  function toggleOverlay(): void {
    overlayVisible = !overlayVisible;
    applyVisibility();
  }

  // --- import / export / apply -------------------------------------------------

  function loadPng(): void {
    if (busy) return;
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = 'image/png';
    input.onchange = () => {
      const file = input.files?.[0];
      if (!file) return;
      const fr = new FileReader();
      fr.onload = () => {
        const dataUrl = fr.result as string; // data: URL — prod CSP blocks blob:
        const img = new Image();
        img.onload = async () => {
          if (validateSkinDimensions(img.naturalWidth, img.naturalHeight) === 'invalid') {
            saveError = $t('skinEditor.invalidImage');
            return;
          }
          if (!viewer) return;
          saveError = null;
          beginStroke(); // so the import is undoable
          // skinview3d upscales legacy 64x32 into the 64x64 canvas itself.
          await viewer.loadSkin(dataUrl, { model: variantToModel(variant) });
          dirty = true;
          syncHistoryFlags();
          renderCompanion();
        };
        img.src = dataUrl;
      };
      fr.readAsDataURL(file);
    };
    input.click();
  }

  function exportPng(): void {
    if (!viewer) return;
    const url = viewer.skinCanvas.toDataURL('image/png');
    const a = document.createElement('a');
    a.href = url;
    a.download = `${account.name}-skin.png`;
    a.click();
  }

  async function apply(): Promise<void> {
    if (busy || !isMicrosoft || !viewer) return;
    busy = true;
    saveError = null;
    applied = false;
    const b64 = viewer.skinCanvas.toDataURL('image/png').split(',')[1];
    const res = await commands.uploadSkin(account.id, b64, variant);
    busy = false;
    if (res.status === 'error') {
      saveError = $t('cosmetics.saveError');
      return;
    }
    applied = true;
    dirty = false;
    onApplied?.(b64, variant);
  }

  function requestClose(): void {
    if (dirty && !window.confirm($t('skinEditor.unsavedConfirm'))) return;
    onClose();
  }

  const TOOLS = [
    { id: 'pencil', icon: 'edit', labelKey: 'skinEditor.toolPencil' },
    { id: 'eraser', icon: 'eraser', labelKey: 'skinEditor.toolEraser' },
    { id: 'eyedropper', icon: 'eyedropper', labelKey: 'skinEditor.toolEyedropper' },
    { id: 'fill', icon: 'fill', labelKey: 'skinEditor.toolFill' },
    { id: 'dodge', icon: 'dodge', labelKey: 'skinEditor.toolDodge' },
    { id: 'burn', icon: 'burn', labelKey: 'skinEditor.toolBurn' },
    { id: 'noise', icon: 'noise', labelKey: 'skinEditor.toolNoise' },
    { id: 'pan', icon: 'hand', labelKey: 'skinEditor.toolPan' },
  ] as const satisfies ReadonlyArray<{ id: Tool; icon: IconName; labelKey: TranslationKey }>;

  const BG_CLASS = {
    dark: 'bg-[#1c1c1f]',
    mid: 'bg-[#4a4a50]',
    light: 'bg-[#c9c9cf]',
  } as const;
</script>

<Modal
  ariaLabelledby="skin-editor-title"
  onClose={requestClose}
  closeOnBackdrop={false}
  panelClass={fullscreen
    ? 'w-[calc(100vw-2rem)] h-[calc(100vh-2rem)] max-w-none p-0 flex flex-col'
    : 'w-[880px] max-w-[calc(100vw-2rem)] max-h-[calc(100vh-2rem)] p-0 flex flex-col'}
>
  <div class="flex items-center px-5 py-3 border-b border-border-subtle shrink-0">
    <div>
      <h3 id="skin-editor-title" class="font-medium text-primary text-base">
        {$t('skinEditor.title')}
      </h3>
      <p class="text-xs text-muted">{account.name}</p>
    </div>
    <div class="ml-auto flex items-center gap-1.5 text-secondary">
      <button
        type="button"
        class="btn-icon btn-icon-sm"
        aria-label={fullscreen ? $t('skinEditor.exitFullscreen') : $t('skinEditor.fullscreen')}
        use:tooltip={fullscreen ? $t('skinEditor.exitFullscreen') : $t('skinEditor.fullscreen')}
        onclick={() => (fullscreen = !fullscreen)}
      >
        <Icon name={fullscreen ? 'shrink' : 'expand'} size={16} />
      </button>
      <button
        type="button"
        class="btn-icon btn-icon-sm"
        aria-label={$t('common.close')}
        onclick={requestClose}
      >
        <Icon name="close" size={18} />
      </button>
    </div>
  </div>

  <div use:observeResizeRow class="flex min-h-0 flex-1">
    <!-- Tool rail -->
    <div class="flex flex-col gap-1 p-2 border-r border-border-subtle text-secondary shrink-0">
      {#each TOOLS as tdef (tdef.id)}
        <button
          type="button"
          class="btn-icon btn-icon-sm"
          class:text-accent={tool === tdef.id}
          aria-pressed={tool === tdef.id}
          aria-label={$t(tdef.labelKey)}
          use:tooltip={$t(tdef.labelKey)}
          onclick={() => (tool = tdef.id)}
        >
          <Icon name={tdef.icon} size={16} />
        </button>
      {/each}
      <div class="h-px bg-border-subtle my-1"></div>
      <button
        type="button"
        class="btn-icon btn-icon-sm"
        class:text-accent={mirror}
        aria-pressed={mirror}
        aria-label={$t('skinEditor.toolMirror')}
        use:tooltip={$t('skinEditor.toolMirror')}
        onclick={toggleMirror}
      >
        <Icon name="mirror" size={16} />
      </button>
      <div class="h-px bg-border-subtle my-1"></div>
      <button
        type="button"
        class="btn-icon btn-icon-sm"
        disabled={!canUndo}
        aria-label={$t('skinEditor.undo')}
        use:tooltip={$t('skinEditor.undo')}
        onclick={undo}
      >
        <Icon name="undo" size={16} />
      </button>
      <button
        type="button"
        class="btn-icon btn-icon-sm"
        disabled={!canRedo}
        aria-label={$t('skinEditor.redo')}
        use:tooltip={$t('skinEditor.redo')}
        onclick={redo}
      >
        <Icon name="redo" size={16} />
      </button>
    </div>

    <!-- 3D viewport + colour -->
    <div class="flex flex-col flex-1 min-w-0 p-3 gap-2">
      <div
        use:observeViewport
        class="rounded-[10px] {BG_CLASS[
          bg
        ]} flex items-center justify-center overflow-hidden flex-1"
      >
        <canvas
          use:mountViewer
          class={tool === 'pan' ? 'cursor-grab active:cursor-grabbing' : 'cursor-crosshair'}
          aria-label={$t('skinEditor.title')}
          onpointerdown={onViewerDown}
          onpointermove={onViewerMove}
          onpointerup={onViewerUp}
          onpointercancel={onViewerUp}
          onpointerleave={clearHover}
        ></canvas>
      </div>
      <span class="text-xs text-muted text-center">{$t('skinEditor.dragToPaint')}</span>
    </div>

    <!-- Draggable splitter: 3D viewport ↔ companion panel. A focusable window
         splitter is a valid ARIA pattern the a11y linter flags as non-interactive. -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={$t('skinEditor.resizeViewport')}
      aria-valuenow={panelWidth}
      aria-valuemin={PANEL_MIN_WIDTH}
      aria-valuemax={maxPanelWidth}
      tabindex={0}
      class="w-1 shrink-0 cursor-col-resize bg-border-subtle hover:bg-border-emphasis focus-visible:bg-accent focus:outline-none"
      onpointerdown={startPanelResize}
      onkeydown={onPanelResizeKey}
    ></div>

    <!-- 2D companion + panel -->
    <div class="flex flex-col p-3 gap-3 shrink-0 overflow-y-auto" style="width:{panelWidth}px">
      <div>
        <div class="flex items-center gap-1.5 mb-1.5">
          <span class="text-xs font-medium text-primary">{$t('skinEditor.companionHeading')}</span>
          <button
            type="button"
            class="btn-icon btn-icon-sm ml-auto"
            class:text-accent={showGrid}
            aria-pressed={showGrid}
            aria-label={$t('skinEditor.grid')}
            use:tooltip={$t('skinEditor.grid')}
            onclick={() => {
              showGrid = !showGrid;
              renderCompanion();
            }}
          >
            <Icon name="grid" size={14} />
          </button>
        </div>
        <div
          use:observeCompanionBox
          class="w-full max-w-[calc(100vh-19rem)] aspect-square mx-auto overflow-auto rounded border border-border-subtle"
        >
          <canvas
            bind:this={companion}
            class="block touch-none {tool === 'pan'
              ? 'cursor-grab active:cursor-grabbing'
              : 'cursor-crosshair'}"
            style="image-rendering:pixelated;aspect-ratio:1/1;width:{companionZoom * 100}%"
            aria-label={$t('skinEditor.companionHeading')}
            onwheel={onCompanionWheel}
            oncontextmenu={(e) => e.preventDefault()}
            onpointerdown={onCompanionDown}
            onpointermove={onCompanionMove}
            onpointerup={onCompanionUp}
            onpointercancel={onCompanionUp}
            onpointerleave={clearHover}
          ></canvas>
        </div>
        <p class="text-[11px] text-muted mt-1">{$t('skinEditor.companionHint')}</p>
      </div>
    </div>
  </div>

  <div class="flex flex-col gap-3 px-5 py-3 border-t border-border-subtle shrink-0">
    <!-- Colour + brush -->
    <div class="flex items-center gap-2 flex-wrap">
      <span class="text-xs text-muted">{$t('skinEditor.colour')}</span>
      <span
        class="w-6 h-6 rounded border border-border-emphasis inline-block"
        style="background:{rgbaToHex(colour)}"
      ></span>
      {#each PALETTE as swatch (rgbaToHex(swatch))}
        <button
          type="button"
          class="w-[18px] h-[18px] rounded border border-border-subtle"
          style="background:{rgbaToHex(swatch)}"
          aria-label={rgbaToHex(swatch)}
          onclick={() => (colour = swatch)}
        ></button>
      {/each}
      <label class="inline-flex items-center gap-1 text-xs text-secondary">
        <input
          type="color"
          value={rgbaToHex(colour)}
          oninput={(e) => (colour = hexToRgba(e.currentTarget.value))}
          aria-label={$t('skinEditor.customColour')}
          class="w-6 h-6 cursor-pointer border-0 bg-transparent p-0"
        />
      </label>
      <span class="text-xs text-muted ml-2">{$t('skinEditor.brushSize')}</span>
      {#each [1, 2, 3] as b (b)}
        <button
          type="button"
          class="w-7 h-7 rounded border inline-flex items-center justify-center {brush === b
            ? 'bg-accent-soft text-accent border-transparent'
            : 'text-secondary border-border-subtle'}"
          aria-pressed={brush === b}
          aria-label={`${$t('skinEditor.brushSize')} ${b}`}
          onclick={() => (brush = b)}
        >
          <span class="rounded-full bg-current" style="width:{b * 3}px;height:{b * 3}px"></span>
        </button>
      {/each}
    </div>

    <!-- Pose · paint layer · visibility · model · background -->
    <div class="flex items-center gap-x-4 gap-y-2 flex-wrap">
      <div class="inline-flex items-center gap-1.5">
        <span class="text-xs text-muted">{$t('skinEditor.poseHeading')}</span>
        {#each POSE_NAMES as p (p)}
          <button
            type="button"
            class="px-2 py-0.5 text-xs rounded border {pose === p
              ? 'bg-accent-soft text-accent border-transparent'
              : 'text-secondary border-border-subtle'}"
            aria-pressed={pose === p}
            onclick={() => setPose(p)}
          >
            {$t(POSE_LABEL[p])}
          </button>
        {/each}
      </div>

      <div class="inline-flex items-center gap-1.5">
        <span class="text-xs text-muted">{$t('skinEditor.paintOn')}</span>
        <div class="inline-flex border border-border-subtle rounded overflow-hidden">
          <button
            type="button"
            class="px-3 py-1 text-xs {activeLayer === 'base'
              ? 'bg-accent-soft text-accent'
              : 'text-secondary'}"
            aria-pressed={activeLayer === 'base'}
            onclick={() => (activeLayer = 'base')}
          >
            {$t('skinEditor.layerBase')}
          </button>
          <button
            type="button"
            class="px-3 py-1 text-xs {activeLayer === 'overlay'
              ? 'bg-accent-soft text-accent'
              : 'text-secondary'}"
            aria-pressed={activeLayer === 'overlay'}
            onclick={() => (activeLayer = 'overlay')}
          >
            {$t('skinEditor.layerOverlay')}
          </button>
        </div>
      </div>

      <div class="inline-flex items-center gap-1.5">
        <span class="text-xs text-muted">{$t('skinEditor.layerVisibility')}</span>
        <button
          type="button"
          class="px-2.5 py-1 text-xs rounded border inline-flex items-center gap-1.5 {baseVisible
            ? 'bg-accent-soft text-accent border-transparent'
            : 'text-secondary border-border-subtle'}"
          aria-pressed={baseVisible}
          onclick={toggleBase}
        >
          <Icon name={baseVisible ? 'eye' : 'eyeOff'} size={13} />
          {$t('skinEditor.layerBase')}
        </button>
        <button
          type="button"
          class="px-2.5 py-1 text-xs rounded border inline-flex items-center gap-1.5 {overlayVisible
            ? 'bg-accent-soft text-accent border-transparent'
            : 'text-secondary border-border-subtle'}"
          aria-pressed={overlayVisible}
          onclick={toggleOverlay}
        >
          <Icon name={overlayVisible ? 'eye' : 'eyeOff'} size={13} />
          {$t('skinEditor.layerOverlay')}
        </button>
      </div>

      <div class="inline-flex items-center gap-1.5">
        <span class="text-xs text-muted">{$t('skinEditor.model')}</span>
        <div class="inline-flex border border-border-subtle rounded overflow-hidden">
          <button
            type="button"
            class="px-3 py-1 text-xs {variant === 'classic'
              ? 'bg-accent-soft text-accent'
              : 'text-secondary'}"
            onclick={() => setVariant('classic')}
          >
            {$t('cosmetics.modelClassic')}
          </button>
          <button
            type="button"
            class="px-3 py-1 text-xs {variant === 'slim'
              ? 'bg-accent-soft text-accent'
              : 'text-secondary'}"
            onclick={() => setVariant('slim')}
          >
            {$t('cosmetics.modelSlim')}
          </button>
        </div>
      </div>

      <div class="inline-flex items-center gap-1 text-xs text-secondary">
        {$t('skinEditor.background')}
        <div class="inline-flex border border-border-subtle rounded overflow-hidden ml-1">
          {#each ['dark', 'mid', 'light'] as const as b (b)}
            <button
              type="button"
              class="w-6 h-5 {BG_CLASS[b]} {bg === b
                ? 'outline outline-2 outline-accent -outline-offset-2'
                : ''}"
              aria-pressed={bg === b}
              aria-label={b}
              onclick={() => (bg = b)}
            ></button>
          {/each}
        </div>
      </div>
    </div>

    <!-- Actions -->
    <div class="flex items-center gap-2">
      <button type="button" class="btn-secondary btn-sm" onclick={loadPng} disabled={busy}>
        <Icon name="upload" size={14} />
        {$t('skinEditor.loadPng')}
      </button>
      <button type="button" class="btn-secondary btn-sm" onclick={exportPng} disabled={busy}>
        <Icon name="download" size={14} />
        {$t('skinEditor.savePng')}
      </button>
      {#if saveError}
        <span class="text-xs text-danger">{saveError}</span>
      {/if}
      {#if applied}
        <span class="text-xs text-success">{$t('skinEditor.applied')}</span>
      {/if}
      {#if isMicrosoft}
        <button type="button" class="btn-primary btn-sm ml-auto" onclick={apply} disabled={busy}>
          {$t('skinEditor.apply')}
        </button>
      {:else}
        <span class="text-xs text-muted ml-auto max-w-[360px]">{$t('skinEditor.offlineHint')}</span>
      {/if}
    </div>
  </div>
</Modal>
