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
  import type { Object3D } from 'three';
  import { SKIN_SIZE, validateSkinDimensions, type Rgba } from '$lib/accounts/skin-editor/buffer';
  import { allFaceRects, faceRectAt, mirrorInFace } from '$lib/accounts/skin-editor/atlas';
  import {
    dodgeBurn,
    eraser,
    fill,
    noise,
    pencil,
    pickColour,
  } from '$lib/accounts/skin-editor/tools';
  import { SkinHistory } from '$lib/accounts/skin-editor/history';
  import { pickTexel } from '$lib/accounts/skin-editor/paint3d';
  import { assertSkinViewerContract } from '$lib/accounts/skin-editor/sv3d-contract';
  import { applyViewerControls } from '$lib/accounts/sv3d-controls';
  import {
    clampPanelWidth,
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
  let showGrid = $state(true);
  let fullscreen = $state(false);
  let bg = $state<'dark' | 'light' | 'mid'>('dark');
  let busy = $state(false);
  let saveError = $state<string | null>(null);
  let applied = $state(false);
  let dirty = $state(false);
  let canUndo = $state(false);
  let canRedo = $state(false);
  let zoom = $state(4); // 2D companion texel size in px (4 / 6 / 8)

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
  let viewerBuilding = false;
  let disposeControls: (() => void) | null = null;
  let viewportBox: HTMLElement | null = null;
  let panelWidth = $state(300);
  let companion: HTMLCanvasElement | null = null;
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
    } finally {
      viewerBuilding = false;
    }
  }

  function mountViewer(node: HTMLCanvasElement) {
    viewerCanvas = node;
    void buildViewer();
    return {
      destroy() {
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

  function startPanelResize(e: PointerEvent): void {
    if (e.button !== 0) return;
    const startX = e.clientX;
    const startWidth = panelWidth;
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const onMove = (ev: PointerEvent): void => {
      // Panel is on the right: dragging left (smaller clientX) widens it.
      panelWidth = clampPanelWidth(startWidth - (ev.clientX - startX));
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
    if (e.key === 'ArrowLeft') panelWidth = clampPanelWidth(panelWidth + PANEL_KEY_STEP);
    else if (e.key === 'ArrowRight') panelWidth = clampPanelWidth(panelWidth - PANEL_KEY_STEP);
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
    paintOne(x, y);
    if (mirror) {
      const m = mirrorInFace(x, y, variant);
      if (m.x !== x || m.y !== y) paintOne(m.x, m.y);
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
    viewerCanvas?.setPointerCapture(e.pointerId);
    beginStroke();
    paintFromViewerEvent(e);
  }

  function onViewerMove(e: PointerEvent): void {
    if (painting) paintFromViewerEvent(e);
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
    const size = SKIN_SIZE * zoom;
    if (companion.width !== size) {
      companion.width = size;
      companion.height = size;
    }
    const c = companion.getContext('2d');
    if (!c) return;
    c.imageSmoothingEnabled = false;
    c.clearRect(0, 0, size, size);
    c.drawImage(viewer.skinCanvas, 0, 0, SKIN_SIZE, SKIN_SIZE, 0, 0, size, size);
    if (showGrid && zoom >= 4) {
      c.strokeStyle = 'rgba(128,128,128,0.25)';
      c.lineWidth = 1;
      c.beginPath();
      for (let i = 1; i < SKIN_SIZE; i++) {
        c.moveTo(i * zoom + 0.5, 0);
        c.lineTo(i * zoom + 0.5, size);
        c.moveTo(0, i * zoom + 0.5);
        c.lineTo(size, i * zoom + 0.5);
      }
      c.stroke();
    }
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
    if (busy || tool === 'pan') return;
    companionPainting = true;
    companion?.setPointerCapture(e.pointerId);
    beginStroke();
    const texel = companionTexel(e);
    if (texel) applyToolAt(texel.x, texel.y);
  }

  function onCompanionMove(e: PointerEvent): void {
    if (!companionPainting) return;
    const texel = companionTexel(e);
    if (texel) applyToolAt(texel.x, texel.y);
  }

  function onCompanionUp(e: PointerEvent): void {
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

  function setZoom(delta: number): void {
    const steps = [4, 6, 8];
    const i = steps.indexOf(zoom);
    zoom = steps[Math.min(steps.length - 1, Math.max(0, i + delta))];
    renderCompanion();
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
    : 'w-[880px] max-w-[calc(100vw-2rem)] p-0 flex flex-col'}
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

  <div class="flex min-h-0 flex-1">
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
        onclick={() => (mirror = !mirror)}
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
        ></canvas>
      </div>
      <span class="text-xs text-muted text-center">{$t('skinEditor.dragToPaint')}</span>
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
            class="px-2 py-0.5 text-xs rounded border {brush === b
              ? 'bg-accent-soft text-accent border-transparent'
              : 'text-secondary border-border-subtle'}"
            aria-pressed={brush === b}
            onclick={() => (brush = b)}
          >
            {b}
          </button>
        {/each}
      </div>
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
      aria-valuemax={PANEL_MAX_WIDTH}
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
          <span class="ml-auto flex gap-0.5">
            <button
              type="button"
              class="btn-icon btn-icon-sm"
              aria-label={$t('skinEditor.zoomOut')}
              use:tooltip={$t('skinEditor.zoomOut')}
              onclick={() => setZoom(-1)}
            >
              <Icon name="zoomOut" size={14} />
            </button>
            <button
              type="button"
              class="btn-icon btn-icon-sm"
              aria-label={$t('skinEditor.zoomIn')}
              use:tooltip={$t('skinEditor.zoomIn')}
              onclick={() => setZoom(1)}
            >
              <Icon name="zoomIn" size={14} />
            </button>
          </span>
        </div>
        <div class="max-h-[264px] max-w-[264px] overflow-auto rounded border border-border-subtle">
          <canvas
            bind:this={companion}
            class="block touch-none"
            style="image-rendering:pixelated"
            aria-label={$t('skinEditor.companionHeading')}
            onpointerdown={onCompanionDown}
            onpointermove={onCompanionMove}
            onpointerup={onCompanionUp}
            onpointercancel={onCompanionUp}
          ></canvas>
        </div>
        <p class="text-[11px] text-muted mt-1">{$t('skinEditor.companionHint')}</p>
      </div>

      <div>
        <div class="text-xs text-muted mb-1">{$t('skinEditor.paintOn')}</div>
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

      <div>
        <div class="text-xs text-muted mb-1">{$t('skinEditor.layerVisibility')}</div>
        <div class="flex gap-1.5">
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
      </div>

      <div>
        <div class="text-xs text-muted mb-1">{$t('skinEditor.model')}</div>
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

      <div class="flex items-center gap-3 flex-wrap">
        <label class="inline-flex items-center gap-1.5 text-xs text-secondary cursor-pointer">
          <input
            type="checkbox"
            checked={showGrid}
            onchange={() => {
              showGrid = !showGrid;
              renderCompanion();
            }}
          />
          {$t('skinEditor.grid')}
        </label>
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
    </div>
  </div>

  <div class="flex items-center gap-2 px-5 py-3 border-t border-border-subtle shrink-0">
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
</Modal>
