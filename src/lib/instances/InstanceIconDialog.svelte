<script lang="ts">
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { pushWarning } from '$lib/toasts/toasts.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import Modal from '$lib/ui/Modal.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { computeCropRect } from './crop';
  import { invalidateInstanceIcon } from './instance-icon-cache';
  import { iconDialog } from './instance-icon-dialog.svelte';

  let { onSaved = () => {} }: { onSaved?: () => void } = $props();

  const FRAME = 256; // on-screen crop frame edge (px)
  const OUT_EDGE = 512; // exported PNG edge; Rust normalizes to 256
  const TITLE_ID = 'instance-icon-dialog-title';

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

  // This component owns the picture mutations; the entry points (Overview
  // avatar, Manage modal) only call iconDialog.pick()/requestRemove(). The
  // handler registration returns its cleanup so a (hypothetical) second mount
  // cannot leave a stale file-input reference behind.
  $effect(() =>
    iconDialog.registerHandlers({
      pick: () => {
        resetState();
        fileInput?.click();
      },
      remove: (id) => {
        void removeIcon(id);
      },
    }),
  );

  async function removeIcon(id: string) {
    const res = await commands.clearInstanceIcon(id);
    if (res.status === 'ok') {
      invalidateInstanceIcon(id);
      onSaved();
    } else {
      pushWarning(formatError(res.error));
    }
  }

  function onFile(e: Event) {
    const file = (e.currentTarget as HTMLInputElement).files?.[0];
    if (!file) return;
    error = null;
    // Read the picked file as a data: URL rather than a blob: URL. The
    // production webview enforces the app CSP (img-src 'self' data: https:),
    // which does not list blob: — a blob: <img src> is blocked there and only
    // works under the dev server, which does not apply the CSP. data: URLs are
    // allowed by the CSP and match the base64/data-URL convention used for
    // every other image in the app (skins, capes, screenshots).
    const reader = new FileReader();
    reader.onload = () => {
      const image = new Image(); // no-network-ok: new Image( decodes a local data: URL
      image.onload = () => {
        imgW = image.naturalWidth;
        imgH = image.naturalHeight;
        minScale = FRAME / Math.min(imgW, imgH);
        scale = minScale;
        offsetX = (FRAME - imgW * scale) / 2;
        offsetY = (FRAME - imgH * scale) / 2;
        img = image;
        // The crop dialog appears only now that there is something to crop.
        if (iconDialog.instanceId) iconDialog.show(iconDialog.instanceId);
      };
      image.onerror = () => {
        // No dialog is open at this point — surface the decode failure as a toast.
        pushWarning($t('instance.icon.errorDecode'));
      };
      image.src = reader.result as string;
    };
    reader.onerror = () => {
      pushWarning($t('instance.icon.errorDecode'));
    };
    reader.readAsDataURL(file);
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
      error = formatError(res.error);
    }
  }
</script>

<!-- Always mounted (outside the Modal) so iconDialog.pick() can open the OS
     file picker synchronously, before any dialog exists. -->
<input
  bind:this={fileInput}
  type="file"
  accept="image/png,image/jpeg,image/webp,image/gif"
  class="hidden"
  onchange={onFile}
/>

{#if iconDialog.open}
  <Modal onClose={close} ariaLabelledby={TITLE_ID} panelClass="w-[19rem] max-w-[90vw] p-4">
    <div class="flex items-center justify-between">
      <h3 id={TITLE_ID} class="text-base font-semibold text-primary">
        {$t('instance.icon.dialogTitle')}
      </h3>
      <CloseButton onClick={close} class="-mr-1 -mt-1" />
    </div>

    {#if img}
      <div class="mt-3 flex flex-col items-center gap-3">
        <div
          class="relative overflow-hidden rounded-xl border border-border-emphasis bg-base"
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
        <div class="flex w-full items-center gap-2">
          <Icon name="zoomOut" size={16} class="shrink-0 text-muted" />
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
          <Icon name="zoomIn" size={16} class="shrink-0 text-muted" />
        </div>
        <p class="text-xs text-muted">{$t('instance.icon.hint')}</p>
      </div>
    {/if}

    <StatusMessage
      tone="danger"
      message={error}
      class="mt-3 rounded border border-danger bg-danger-bg p-2"
    />

    {#if img}
      <div class="mt-3 flex justify-end">
        <button type="button" class="btn-primary btn-sm" disabled={busy} onclick={save}>
          {$t('common.save')}
        </button>
      </div>
    {/if}
  </Modal>
{/if}
