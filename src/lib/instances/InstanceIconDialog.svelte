<script lang="ts">
  import { untrack } from 'svelte';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import Modal from '$lib/ui/Modal.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import { computeCropRect } from './crop';
  import { invalidateInstanceIcon, loadInstanceIcon } from './instance-icon-cache';
  import { iconDialog } from './instance-icon-dialog.svelte';

  let { onSaved = () => {} }: { onSaved?: () => void } = $props();

  const FRAME = 256; // on-screen crop frame edge (px)
  const OUT_EDGE = 512; // exported PNG edge; Rust normalizes to 256
  const TITLE_ID = 'instance-icon-dialog-title';

  let fileInput = $state<HTMLInputElement>();
  // The instance's currently-stored picture (data URL), shown as a preview when
  // the dialog opens on an instance that already has one. `null` = none / not
  // loaded yet. Distinct from `img`, which is a freshly-picked source to crop.
  let existingUrl = $state<string | null>(null);
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

  // Blob URL for the currently-loaded source image. It must stay alive while
  // the preview <img src={img.src}> is displayed; revoking it in the loader's
  // onload (as before) left the displayed <img> pointing at a dead URL and
  // showed a broken image. Revoked when replaced by a new pick, or on reset.
  let objectUrl: string | null = null;

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
    if (objectUrl) {
      URL.revokeObjectURL(objectUrl);
      objectUrl = null;
    }
    if (fileInput) fileInput.value = '';
  }

  function close() {
    iconDialog.close();
    resetState();
    existingUrl = null;
  }

  // On open (or target-instance change), start from a clean crop session and,
  // if the instance already has a picture, load it for preview. `open` /
  // `instanceId` / `hasIcon` only change on show()/close() — never while the
  // user is actively cropping — so this cannot wipe a freshly picked image
  // mid-edit.
  $effect(() => {
    const open = iconDialog.open;
    const id = iconDialog.instanceId;
    const has = iconDialog.hasIcon;
    untrack(() => {
      resetState();
      existingUrl = null;
      if (open && has && id) {
        loadInstanceIcon(id).then((url) => {
          // Ignore a stale resolve if the dialog closed or switched instance.
          if (iconDialog.open && iconDialog.instanceId === id) existingUrl = url;
        });
      }
    });
  });

  function onFile(e: Event) {
    const file = (e.currentTarget as HTMLInputElement).files?.[0];
    if (!file) return;
    error = null;
    // Replace any previously-loaded source; revoke its blob URL first.
    if (objectUrl) URL.revokeObjectURL(objectUrl);
    const url = URL.createObjectURL(file);
    objectUrl = url;
    const image = new Image();
    image.onload = () => {
      imgW = image.naturalWidth;
      imgH = image.naturalHeight;
      minScale = FRAME / Math.min(imgW, imgH);
      scale = minScale;
      offsetX = (FRAME - imgW * scale) / 2;
      offsetY = (FRAME - imgH * scale) / 2;
      img = image;
      // Do NOT revoke here: the preview <img src={img.src}> still needs this
      // blob URL. It is revoked on reset/close or when a new file replaces it.
    };
    image.onerror = () => {
      error = $t('instance.icon.errorDecode');
      if (objectUrl === url) {
        URL.revokeObjectURL(url);
        objectUrl = null;
      }
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
      error = formatError(res.error);
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
      error = formatError(res.error);
    }
  }
</script>

{#if iconDialog.open}
  <Modal onClose={close} ariaLabelledby={TITLE_ID} panelClass="w-[22rem] max-w-[90vw] p-5">
    <h3 id={TITLE_ID} class="text-base font-semibold text-primary">
      {$t('instance.icon.dialogTitle')}
    </h3>

    {#if img}
      <!-- Crop a freshly-picked source. -->
      <div class="mt-4 flex flex-col items-center gap-3">
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
    {:else if existingUrl}
      <!-- The current picture. The image itself is the "change" affordance
           (hover/focus reveals a scrim + camera glyph — Discord/Slack pattern);
           the corner trash badge removes it. Siblings, not nested, so both stay
           real buttons. -->
      <div class="mt-4 flex justify-center">
        <div class="relative">
          <button
            type="button"
            class="group relative block overflow-hidden rounded-2xl border border-border-subtle
              shadow-sm focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent
              focus-visible:outline-offset-2"
            onclick={() => fileInput?.click()}
            aria-label={$t('instance.icon.editTooltip')}
            use:tooltip={$t('instance.icon.editTooltip')}
          >
            <img src={existingUrl} alt="" draggable="false" class="h-44 w-44 object-cover" />
            <span
              class="absolute inset-0 flex items-center justify-center bg-black/45 opacity-0
                transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100"
              aria-hidden="true"
            >
              <Icon name="camera" size={26} class="text-white" />
            </span>
          </button>
          <button
            type="button"
            class="btn-icon btn-icon-sm btn-icon-danger absolute -right-2.5 -top-2.5 z-10
              rounded-full border border-border-subtle bg-surface/90 shadow-sm"
            disabled={busy}
            onclick={remove}
            aria-label={$t('instance.icon.remove')}
            use:tooltip={$t('instance.icon.remove')}
          >
            <Icon name="trash" size={14} />
          </button>
        </div>
      </div>
    {:else}
      <!-- Empty: click-to-browse drop zone. -->
      <button
        type="button"
        class="mt-4 flex w-full flex-col items-center justify-center gap-2 rounded-xl border-2
          border-dashed border-border-emphasis px-4 py-10 text-center transition-colors
          hover:border-accent"
        onclick={() => fileInput?.click()}
      >
        <Icon name="upload" size={26} class="text-muted" />
        <span class="text-sm font-medium text-secondary">{$t('instance.icon.chooseFile')}</span>
        <span class="text-xs text-muted">PNG · JPG · WebP · GIF</span>
      </button>
    {/if}

    <StatusMessage
      tone="danger"
      message={error}
      class="mt-3 rounded border border-danger bg-danger-bg p-2"
    />

    <div class="mt-4 flex justify-end gap-2">
      <button type="button" class="btn-secondary btn-sm" disabled={busy} onclick={close}>
        {img ? $t('common.cancel') : $t('common.close')}
      </button>
      {#if img}
        <button type="button" class="btn-primary btn-sm" disabled={busy} onclick={save}>
          {$t('common.save')}
        </button>
      {/if}
    </div>

    <input
      bind:this={fileInput}
      type="file"
      accept="image/png,image/jpeg,image/webp,image/gif"
      class="hidden"
      onchange={onFile}
    />
  </Modal>
{/if}
