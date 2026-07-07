<script lang="ts">
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';

  // Zoom + pan + freehand annotation over a single screenshot. Strokes live on
  // a <canvas> sized to the image's natural (preview) pixels and drawn in that
  // pixel space, so they stay pinned to image content as you zoom/pan (the image
  // and canvas share one CSS transform). Annotations are ephemeral — cleared
  // when the image changes. `overlayDataUrl()` exposes the strokes as a
  // transparent PNG for the (backend) "save annotated copy" flow.
  let { url }: { url: string } = $props();

  type Tool = 'pan' | 'marker' | 'eraser' | 'crop';
  let tool = $state<Tool>('pan');

  const COLORS = [
    { key: 'screenshots.colorRed', css: '#ef4444' },
    { key: 'screenshots.colorOrange', css: '#f97316' },
    { key: 'screenshots.colorYellow', css: '#eab308' },
    { key: 'screenshots.colorGreen', css: '#22c55e' },
    { key: 'screenshots.colorBlue', css: '#3b82f6' },
    { key: 'screenshots.colorWhite', css: '#ffffff' },
  ] as const;
  let color = $state<string>(COLORS[0].css);
  let thick = $state(false);

  const MIN_Z = 1;
  const MAX_Z = 8;
  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);

  type Point = { x: number; y: number };
  type Stroke = { color: string; width: number; erase: boolean; points: Point[] };
  let strokes = $state<Stroke[]>([]);

  let wrapEl = $state<HTMLDivElement | null>(null);
  let imgEl = $state<HTMLImageElement | null>(null);
  let canvasEl = $state<HTMLCanvasElement | null>(null);
  let drawing = false;
  let panning = $state(false);
  let lastPan: Point | null = null;

  // Crop selection in image-pixel coords. Feeds the "save annotated copy" flow;
  // outside the rect is dimmed as a preview. `natW/natH` are the image's natural
  // (preview) pixel dimensions, used for the crop overlay's SVG viewBox.
  type Rect = { x: number; y: number; w: number; h: number };
  let crop = $state<Rect | null>(null);
  let cropping = false;
  let cropStart: Point | null = null;
  let natW = $state(0);
  let natH = $state(0);
  let cropApplied = $state(false);

  // The image region currently shown: the applied crop, else the whole image.
  // In crop mode we always show the full image so a new selection can be drawn.
  const full = $derived<Rect>({ x: 0, y: 0, w: natW || 1, h: natH || 1 });
  const src = $derived<Rect>(tool === 'crop' ? full : crop && cropApplied ? crop : full);
  const layout = $derived(
    natW && natH
      ? {
          w: (natW / src.w) * 100,
          h: (natH / src.h) * 100,
          left: (-src.x / src.w) * 100,
          top: (-src.y / src.h) * 100,
        }
      : { w: 100, h: 100, left: 0, top: 0 },
  );

  // New image → blank slate at 1×.
  $effect(() => {
    void url;
    strokes = [];
    crop = null;
    cropApplied = false;
    zoom = 1;
    panX = 0;
    panY = 0;
  });

  function onImgLoad() {
    if (!imgEl || !canvasEl) return;
    natW = imgEl.naturalWidth;
    natH = imgEl.naturalHeight;
    canvasEl.width = natW;
    canvasEl.height = natH;
    redraw();
  }

  function redraw() {
    const ctx = canvasEl?.getContext('2d');
    if (!ctx || !canvasEl) return;
    ctx.clearRect(0, 0, canvasEl.width, canvasEl.height);
    ctx.lineJoin = 'round';
    ctx.lineCap = 'round';
    for (const s of strokes) {
      if (s.points.length === 0) continue;
      ctx.globalCompositeOperation = s.erase ? 'destination-out' : 'source-over';
      ctx.strokeStyle = s.erase ? 'rgba(0,0,0,1)' : s.color;
      ctx.lineWidth = s.width;
      ctx.beginPath();
      ctx.moveTo(s.points[0].x, s.points[0].y);
      for (let i = 1; i < s.points.length; i++) ctx.lineTo(s.points[i].x, s.points[i].y);
      ctx.stroke();
    }
  }

  // Pointer client coords → canvas (image) pixel coords. getBoundingClientRect()
  // already reflects the live zoom/pan, so this maps correctly at any transform.
  function toCanvas(e: PointerEvent): Point {
    const rect = canvasEl!.getBoundingClientRect();
    return {
      x: ((e.clientX - rect.left) / rect.width) * canvasEl!.width,
      y: ((e.clientY - rect.top) / rect.height) * canvasEl!.height,
    };
  }

  // Stroke width as a fraction of the image's long edge, so it feels consistent
  // regardless of the screenshot's resolution.
  function strokeWidth(): number {
    if (!canvasEl) return 4;
    return (thick ? 0.01 : 0.004) * Math.max(canvasEl.width, canvasEl.height);
  }

  function onPointerDown(e: PointerEvent) {
    if (tool === 'crop') {
      cropping = true;
      cropStart = toCanvas(e);
      crop = { x: cropStart.x, y: cropStart.y, w: 0, h: 0 };
      canvasEl?.setPointerCapture(e.pointerId);
      return;
    }
    if (tool === 'pan') {
      if (zoom <= 1) return;
      panning = true;
      lastPan = { x: e.clientX, y: e.clientY };
      canvasEl?.setPointerCapture(e.pointerId);
      return;
    }
    drawing = true;
    canvasEl?.setPointerCapture(e.pointerId);
    strokes = [
      ...strokes,
      { color, width: strokeWidth(), erase: tool === 'eraser', points: [toCanvas(e)] },
    ];
    redraw();
  }

  function onPointerMove(e: PointerEvent) {
    if (cropping && cropStart) {
      crop = normRect(cropStart, toCanvas(e));
      return;
    }
    if (panning && lastPan) {
      panX += e.clientX - lastPan.x;
      panY += e.clientY - lastPan.y;
      lastPan = { x: e.clientX, y: e.clientY };
      return;
    }
    if (!drawing) return;
    strokes[strokes.length - 1].points.push(toCanvas(e));
    redraw();
  }

  function onPointerUp() {
    if (cropping) {
      cropping = false;
      cropStart = null;
      // Discard an accidental tiny selection (treat as no crop).
      if (crop && (crop.w < 8 || crop.h < 8)) crop = null;
    }
    drawing = false;
    panning = false;
    lastPan = null;
  }

  function clamp(v: number, max: number): number {
    return Math.max(0, Math.min(max, v));
  }
  function normRect(a: Point, b: Point): Rect {
    const x1 = clamp(Math.min(a.x, b.x), natW);
    const y1 = clamp(Math.min(a.y, b.y), natH);
    const x2 = clamp(Math.max(a.x, b.x), natW);
    const y2 = clamp(Math.max(a.y, b.y), natH);
    return { x: x1, y: y1, w: x2 - x1, h: y2 - y1 };
  }

  function undo() {
    strokes = strokes.slice(0, -1);
    redraw();
  }
  function clearAll() {
    strokes = [];
    crop = null;
    cropApplied = false;
    redraw();
  }
  function applyCrop() {
    if (!crop) return;
    cropApplied = true;
    tool = 'pan';
    resetZoom();
  }
  function cancelCrop() {
    crop = null;
    cropApplied = false;
    tool = 'pan';
  }

  function clampZoom(z: number) {
    return Math.min(MAX_Z, Math.max(MIN_Z, z));
  }
  // Zoom about a screen point, keeping that point fixed (transform-origin center).
  function zoomAt(clientX: number, clientY: number, factor: number) {
    if (!wrapEl) return;
    const next = clampZoom(zoom * factor);
    const applied = next / zoom;
    const r = wrapEl.getBoundingClientRect();
    const cx = clientX - r.left - r.width / 2;
    const cy = clientY - r.top - r.height / 2;
    panX = cx - (cx - panX) * applied;
    panY = cy - (cy - panY) * applied;
    zoom = next;
    if (zoom === 1) {
      panX = 0;
      panY = 0;
    }
  }
  function onWheel(e: WheelEvent) {
    e.preventDefault();
    zoomAt(e.clientX, e.clientY, e.deltaY < 0 ? 1.15 : 1 / 1.15);
  }
  function zoomButton(factor: number) {
    if (!wrapEl) return;
    const r = wrapEl.getBoundingClientRect();
    zoomAt(r.left + r.width / 2, r.top + r.height / 2, factor);
  }
  function resetZoom() {
    zoom = 1;
    panX = 0;
    panY = 0;
  }

  function onKey(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && (e.key === 'z' || e.key === 'Z')) {
      e.preventDefault();
      undo();
    }
  }

  // Exposed for the parent's "save annotated copy" flow (chunk 2).
  export function hasStrokes(): boolean {
    return strokes.length > 0;
  }
  export function overlayDataUrl(): string | null {
    return strokes.length ? (canvasEl?.toDataURL('image/png') ?? null) : null;
  }
  export function cropRect(): Rect | null {
    return crop && cropApplied ? crop : null;
  }

  const cursor = $derived(
    tool === 'pan' ? (panning ? 'grabbing' : zoom > 1 ? 'grab' : 'default') : 'crosshair',
  );
</script>

<svelte:window onkeydown={onKey} />

<div class="relative z-10 flex flex-col items-center gap-3">
  <div
    class="flex flex-wrap items-center justify-center gap-1.5 rounded-full border border-border-subtle bg-surface px-3 py-1.5 text-secondary"
  >
    <button
      type="button"
      class="btn-icon btn-icon-sm"
      class:text-accent={tool === 'pan'}
      aria-pressed={tool === 'pan'}
      aria-label={$t('screenshots.toolPan')}
      use:tooltip={$t('screenshots.toolPan')}
      onclick={() => (tool = 'pan')}><Icon name="hand" size={16} /></button
    >
    <button
      type="button"
      class="btn-icon btn-icon-sm"
      class:text-accent={tool === 'marker'}
      aria-pressed={tool === 'marker'}
      aria-label={$t('screenshots.toolMarker')}
      use:tooltip={$t('screenshots.toolMarker')}
      onclick={() => (tool = 'marker')}><Icon name="marker" size={16} /></button
    >
    <button
      type="button"
      class="btn-icon btn-icon-sm"
      class:text-accent={tool === 'eraser'}
      aria-pressed={tool === 'eraser'}
      aria-label={$t('screenshots.toolEraser')}
      use:tooltip={$t('screenshots.toolEraser')}
      onclick={() => (tool = 'eraser')}><Icon name="eraser" size={16} /></button
    >
    <button
      type="button"
      class="btn-icon btn-icon-sm"
      class:text-accent={tool === 'crop'}
      aria-pressed={tool === 'crop'}
      aria-label={$t('screenshots.toolCrop')}
      use:tooltip={$t('screenshots.toolCrop')}
      onclick={() => {
        tool = 'crop';
        cropApplied = false;
        resetZoom();
      }}><Icon name="crop" size={16} /></button
    >

    <span class="mx-0.5 h-4 w-px bg-border-subtle"></span>

    {#each COLORS as c (c.css)}
      <button
        type="button"
        class="h-5 w-5 rounded-full border border-border-subtle transition-transform hover:scale-110"
        class:ring-2={color === c.css}
        class:ring-accent={color === c.css}
        style="background-color: {c.css};"
        aria-pressed={color === c.css}
        aria-label={$t(c.key)}
        use:tooltip={$t(c.key)}
        onclick={() => {
          color = c.css;
          tool = 'marker';
        }}
      ></button>
    {/each}

    <button
      type="button"
      class="btn-icon btn-icon-sm"
      class:text-accent={thick}
      aria-pressed={thick}
      aria-label={$t(thick ? 'screenshots.thick' : 'screenshots.thin')}
      use:tooltip={$t(thick ? 'screenshots.thick' : 'screenshots.thin')}
      onclick={() => (thick = !thick)}
    >
      <span
        class="inline-block rounded-full bg-current"
        style={thick ? 'width:12px;height:12px' : 'width:6px;height:6px'}
      ></span>
    </button>

    <span class="mx-0.5 h-4 w-px bg-border-subtle"></span>

    <button
      type="button"
      class="btn-icon btn-icon-sm"
      aria-label={$t('screenshots.undo')}
      use:tooltip={$t('screenshots.undo')}
      disabled={strokes.length === 0}
      onclick={undo}><Icon name="undo" size={16} /></button
    >
    <button
      type="button"
      class="btn-icon btn-icon-sm btn-icon-danger"
      aria-label={$t('screenshots.clearAll')}
      use:tooltip={$t('screenshots.clearAll')}
      disabled={strokes.length === 0 && !crop && !cropApplied}
      onclick={clearAll}><Icon name="trash" size={16} /></button
    >

    <span class="mx-0.5 h-4 w-px bg-border-subtle"></span>

    <button
      type="button"
      class="btn-icon btn-icon-sm"
      aria-label={$t('screenshots.zoomOut')}
      use:tooltip={$t('screenshots.zoomOut')}
      disabled={zoom <= MIN_Z}
      onclick={() => zoomButton(1 / 1.3)}><Icon name="zoomOut" size={16} /></button
    >
    <button
      type="button"
      class="min-w-[3rem] text-center text-xs text-primary"
      aria-label={$t('screenshots.resetZoom')}
      use:tooltip={$t('screenshots.resetZoom')}
      onclick={resetZoom}>{Math.round(zoom * 100)}%</button
    >
    <button
      type="button"
      class="btn-icon btn-icon-sm"
      aria-label={$t('screenshots.zoomIn')}
      use:tooltip={$t('screenshots.zoomIn')}
      disabled={zoom >= MAX_Z}
      onclick={() => zoomButton(1.3)}><Icon name="zoomIn" size={16} /></button
    >
  </div>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={wrapEl}
    class="relative max-h-[64vh] max-w-[88vw] overflow-hidden rounded"
    style="aspect-ratio: {src.w} / {src.h};"
    onwheel={onWheel}
  >
    <div
      class="absolute inset-0"
      style="transform: translate({panX}px, {panY}px) scale({zoom}); transform-origin: center center;"
    >
      <img
        bind:this={imgEl}
        src={url}
        alt=""
        draggable="false"
        onload={onImgLoad}
        class="pointer-events-none absolute max-w-none select-none"
        style="width: {layout.w}%; height: {layout.h}%; left: {layout.left}%; top: {layout.top}%;"
      />
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <canvas
        bind:this={canvasEl}
        class="absolute max-w-none"
        style="width: {layout.w}%; height: {layout.h}%; left: {layout.left}%; top: {layout.top}%; touch-action: none; cursor: {cursor};"
        onpointerdown={onPointerDown}
        onpointermove={onPointerMove}
        onpointerup={onPointerUp}
        onpointercancel={onPointerUp}
      ></canvas>

      {#if tool === 'crop' && crop && natW && natH}
        <svg
          class="pointer-events-none absolute inset-0 h-full w-full"
          viewBox="0 0 {natW} {natH}"
          preserveAspectRatio="none"
        >
          <rect x="0" y="0" width={natW} height={crop.y} fill="rgba(0,0,0,0.5)" />
          <rect
            x="0"
            y={crop.y + crop.h}
            width={natW}
            height={natH - crop.y - crop.h}
            fill="rgba(0,0,0,0.5)"
          />
          <rect x="0" y={crop.y} width={crop.x} height={crop.h} fill="rgba(0,0,0,0.5)" />
          <rect
            x={crop.x + crop.w}
            y={crop.y}
            width={natW - crop.x - crop.w}
            height={crop.h}
            fill="rgba(0,0,0,0.5)"
          />
          <rect
            x={crop.x}
            y={crop.y}
            width={crop.w}
            height={crop.h}
            fill="none"
            stroke="white"
            stroke-width="2"
            vector-effect="non-scaling-stroke"
          />
        </svg>

        <!-- Apply / cancel, anchored under the selection's bottom edge. -->
        <div
          class="absolute flex gap-1 rounded-full border border-border-subtle bg-surface p-1"
          style="left: {((crop.x + crop.w / 2) / natW) * 100}%; top: {((crop.y + crop.h) / natH) *
            100}%; transform: translate(-50%, 6px) scale({1 / zoom}); transform-origin: top center;"
        >
          <button
            type="button"
            class="btn-icon btn-icon-sm btn-icon-success"
            aria-label={$t('screenshots.applyCrop')}
            use:tooltip={$t('screenshots.applyCrop')}
            onclick={applyCrop}><Icon name="success" size={16} /></button
          >
          <button
            type="button"
            class="btn-icon btn-icon-sm btn-icon-danger"
            aria-label={$t('screenshots.cancelCrop')}
            use:tooltip={$t('screenshots.cancelCrop')}
            onclick={cancelCrop}><Icon name="close" size={16} /></button
          >
        </div>
      {/if}
    </div>
  </div>
</div>
