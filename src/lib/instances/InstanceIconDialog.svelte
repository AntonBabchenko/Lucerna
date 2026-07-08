<script lang="ts">
  import { commands } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { computeCropRect } from './crop';
  import { invalidateInstanceIcon } from './instance-icon-cache';
  import { iconDialog } from './instance-icon-dialog.svelte';

  let { onSaved = () => {} }: { onSaved?: () => void } = $props();

  const FRAME = 256; // on-screen crop frame edge (px)
  const OUT_EDGE = 512; // exported PNG edge; Rust normalizes to 256

  let fileInput = $state<HTMLInputElement>();
  let img: HTMLImageElement | null = $state(null);
  let imgW = $state(0);
  let imgH = $state(0);
  let minScale = $state(1);
  let scale = $state(1);
  let offsetX = $state(0);
  let offsetY = $state(0);
  let busy = $state(false);
  let error = $state<string | null>(null);

  let dragging = false;
  let dragX = 0;
  let dragY = 0;
  let startX = 0;
  let startY = 0;

  function resetState() {
    img = null;
    imgW = 0;
    imgH = 0;
    minScale = 1;
    scale = 1;
    offsetX = 0;
    offsetY = 0;
    busy = false;
    error = null;
    if (fileInput) fileInput.value = '';
  }

  function close() {
    iconDialog.close();
    resetState();
  }

  function onKeydown(e: KeyboardEvent) {
    if (iconDialog.open && e.key === 'Escape') close();
  }

  function onFile(e: Event) {
    const file = (e.currentTarget as HTMLInputElement).files?.[0];
    if (!file) return;
    error = null;
    const url = URL.createObjectURL(file);
    const image = new Image();
    image.onload = () => {
      imgW = image.naturalWidth;
      imgH = image.naturalHeight;
      minScale = FRAME / Math.min(imgW, imgH);
      scale = minScale;
      offsetX = (FRAME - imgW * scale) / 2;
      offsetY = (FRAME - imgH * scale) / 2;
      img = image;
      URL.revokeObjectURL(url);
    };
    image.onerror = () => {
      error = $t('instance.icon.errorDecode');
      URL.revokeObjectURL(url);
    };
    image.src = url;
  }

  function clampOffsets() {
    const w = imgW * scale;
    const h = imgH * scale;
    offsetX = Math.min(0, Math.max(FRAME - w, offsetX));
    offsetY = Math.min(0, Math.max(FRAME - h, offsetY));
  }

  function zoomTo(next: number) {
    const prev = scale;
    scale = Math.min(minScale * 8, Math.max(minScale, next));
    const c = FRAME / 2;
    offsetX = c - (c - offsetX) * (scale / prev);
    offsetY = c - (c - offsetY) * (scale / prev);
    clampOffsets();
  }

  function onWheel(e: WheelEvent) {
    if (!img) return;
    e.preventDefault();
    zoomTo(scale * (e.deltaY < 0 ? 1.08 : 1 / 1.08));
  }

  function onPointerDown(e: PointerEvent) {
    if (!img) return;
    dragging = true;
    dragX = e.clientX;
    dragY = e.clientY;
    startX = offsetX;
    startY = offsetY;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragging) return;
    offsetX = startX + (e.clientX - dragX);
    offsetY = startY + (e.clientY - dragY);
    clampOffsets();
  }

  function onPointerUp() {
    dragging = false;
  }

  async function save() {
    const id = iconDialog.instanceId;
    if (!img || !id) return;
    busy = true;
    error = null;
    const rect = computeCropRect({ imgW, imgH, scale, offsetX, offsetY, frame: FRAME });
    const canvas = document.createElement('canvas');
    canvas.width = OUT_EDGE;
    canvas.height = OUT_EDGE;
    const ctx = canvas.getContext('2d');
    if (!ctx) {
      busy = false;
      error = $t('instance.icon.errorSave');
      return;
    }
    ctx.drawImage(img, rect.sx, rect.sy, rect.sSize, rect.sSize, 0, 0, OUT_EDGE, OUT_EDGE);
    const dataUrl = canvas.toDataURL('image/png');
    const b64 = dataUrl.slice(dataUrl.indexOf(',') + 1);
    const res = await commands.setInstanceIcon(id, b64);
    busy = false;
    if (res.status === 'ok') {
      invalidateInstanceIcon(id);
      onSaved();
      close();
    } else {
      error = $t('instance.icon.errorSave');
    }
  }

  async function remove() {
    const id = iconDialog.instanceId;
    if (!id) return;
    busy = true;
    error = null;
    const res = await commands.clearInstanceIcon(id);
    busy = false;
    if (res.status === 'ok') {
      invalidateInstanceIcon(id);
      onSaved();
      close();
    } else {
      error = $t('instance.icon.errorSave');
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if iconDialog.open}
  <div class="fixed inset-0 z-[60] flex items-center justify-center">
    <button
      type="button"
      class="absolute inset-0 bg-black/50"
      aria-label={$t('common.cancel')}
      onclick={close}
    ></button>
    <div
      class="relative z-10 w-[22rem] max-w-[90vw] rounded-xl border border-border-subtle bg-base p-5 shadow-xl"
      role="dialog"
      aria-modal="true"
      aria-label={$t('instance.icon.dialogTitle')}
    >
      <h3 class="mb-3 text-base font-semibold text-primary">{$t('instance.icon.dialogTitle')}</h3>

      {#if img}
        <div class="flex flex-col items-center gap-3">
          <div
            class="relative overflow-hidden rounded-xl border border-border-subtle bg-base"
            style="width:{FRAME}px;height:{FRAME}px;touch-action:none;cursor:grab"
            onwheel={onWheel}
            onpointerdown={onPointerDown}
            onpointermove={onPointerMove}
            onpointerup={onPointerUp}
            onpointercancel={onPointerUp}
            role="presentation"
          >
            <img
              src={img.src}
              alt=""
              draggable="false"
              class="absolute left-0 top-0 max-w-none select-none"
              style="width:{imgW * scale}px;height:{imgH *
                scale}px;transform:translate({offsetX}px,{offsetY}px)"
            />
          </div>
          <input
            type="range"
            class="w-full"
            min={minScale}
            max={minScale * 8}
            step="0.001"
            value={scale}
            aria-label={$t('instance.icon.zoom')}
            oninput={(e) => zoomTo(Number((e.currentTarget as HTMLInputElement).value))}
          />
          <p class="text-xs text-secondary">{$t('instance.icon.hint')}</p>
        </div>
      {:else}
        <button type="button" class="btn-secondary w-full" onclick={() => fileInput?.click()}>
          {$t('instance.icon.chooseFile')}
        </button>
      {/if}

      {#if error}
        <p class="mt-3 text-sm text-danger">{error}</p>
      {/if}

      <div class="mt-4 flex items-center justify-between">
        <div>
          {#if iconDialog.hasIcon}
            <button type="button" class="btn-ghost-danger btn-sm" disabled={busy} onclick={remove}>
              {$t('instance.icon.remove')}
            </button>
          {/if}
        </div>
        <div class="flex gap-2">
          <button type="button" class="btn-secondary btn-sm" disabled={busy} onclick={close}>
            {$t('common.cancel')}
          </button>
          <button type="button" class="btn-primary btn-sm" disabled={!img || busy} onclick={save}>
            {$t('common.save')}
          </button>
        </div>
      </div>

      <input
        bind:this={fileInput}
        type="file"
        accept="image/png,image/jpeg,image/webp,image/gif"
        class="hidden"
        onchange={onFile}
      />
    </div>
  </div>
{/if}
