<script lang="ts">
  import { onDestroy } from 'svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { Icon } from '$lib/ui/icons';
  import { t } from '$lib/i18n';
  import { commands, type Account, type CapeInfo, type SkinVariant } from '$lib/ipc/bindings';
  import { drawCapeFront } from '$lib/accounts/cape-render';
  import type { SkinViewer } from 'skinview3d';

  let { account, onClose }: { account: Account; onClose: () => void } = $props();

  let loading = $state(true);
  let loadError = $state(false);
  let saveError = $state<string | null>(null);
  let busy = $state(false);
  let capes = $state<CapeInfo[]>([]);
  let variant = $state<SkinVariant>('classic');
  // Base64 of the currently-displayed skin, kept so a reset can snapshot it.
  let currentSkinB64: string | null = null;
  // Two-step guard for the destructive reset, plus an in-session undo snapshot
  // of the skin that was active before the reset (so it can be re-applied).
  let confirmingReset = $state(false);
  let undoSkin = $state<{ b64: string; variant: SkinVariant } | null>(null);

  // 3D preview (skinview3d, lazily imported so Three.js stays out of the main
  // bundle). WebGL is imperative, not reactive: we drive it via explicit calls
  // and dispose it when the canvas unmounts.
  let viewerCanvas: HTMLCanvasElement | null = null;
  let viewer: SkinViewer | null = null;
  let viewerBuilding = false;

  const activeCape = $derived(capes.find((c) => c.is_active) ?? null);
  const activeCapeName = $derived(activeCape ? (activeCape.alias ?? activeCape.id) : null);
  const noCapeActive = $derived(activeCape === null);

  const skinDataUrl = () => (currentSkinB64 !== null ? `data:image/png;base64,${currentSkinB64}` : null);
  const modelOf = (v: SkinVariant): 'default' | 'slim' => (v === 'slim' ? 'slim' : 'default');

  // Build the viewer once we have BOTH a mounted canvas and a skin. Callable
  // from either trigger (canvas-mount action or skin-fetch); idempotent via the
  // `viewer`/`viewerBuilding` guards, so whichever arrives last wins the race.
  async function ensureViewer() {
    if (viewer || viewerBuilding || !viewerCanvas || currentSkinB64 === null) return;
    viewerBuilding = true;
    try {
      const skinview3d = await import('skinview3d');
      const url = skinDataUrl();
      // The modal may have closed (canvas unmounted) during the async import.
      if (!viewerCanvas || url === null) return;
      viewer = new skinview3d.SkinViewer({
        canvas: viewerCanvas,
        width: 200,
        height: 260,
        skin: url,
        model: modelOf(variant),
      });
      viewer.controls.enableZoom = false;
      viewer.controls.enablePan = false;
      viewer.autoRotate = true;
      viewer.autoRotateSpeed = 0.5;
      viewer.animation = new skinview3d.IdleAnimation();
      await applyCapeToViewer();
    } finally {
      viewerBuilding = false;
    }
  }

  async function applySkinToViewer() {
    const url = skinDataUrl();
    if (!viewer || url === null) return;
    await viewer.loadSkin(url, { model: modelOf(variant) });
  }

  async function applyCapeToViewer() {
    if (!viewer) return;
    const cape = activeCape;
    if (!cape) {
      viewer.loadCape(null);
      return;
    }
    const res = await commands.capeTexture(cape.texture_url);
    if (!viewer) return;
    if (res.status === 'ok' && res.data) {
      await viewer.loadCape(`data:image/png;base64,${res.data}`);
    } else {
      viewer.loadCape(null);
    }
  }

  function syncSkinPreview() {
    if (viewer) void applySkinToViewer();
    else void ensureViewer();
  }

  function mountViewer(node: HTMLCanvasElement) {
    viewerCanvas = node;
    void ensureViewer();
    return {
      destroy() {
        viewer?.dispose();
        viewer = null;
        viewerCanvas = null;
      },
    };
  }

  onDestroy(() => {
    viewer?.dispose();
    viewer = null;
  });

  async function refreshSkin() {
    const skinRes = await commands.accountSkin(account.uuid);
    currentSkinB64 = skinRes.status === 'ok' && skinRes.data ? skinRes.data.skin_png_base64 : null;
    syncSkinPreview();
  }

  async function load(showLoading = true) {
    if (showLoading) loading = true;
    loadError = false;
    const res = await commands.getCosmetics(account.id);
    if (res.status === 'error') {
      loadError = true;
      loading = false;
      return;
    }
    capes = res.data.capes;
    variant = res.data.active_variant;
    loading = false;
    await refreshSkin();
  }

  function renderCape(node: HTMLCanvasElement, url: string) {
    void (async () => {
      const res = await commands.capeTexture(url);
      if (res.status !== 'ok' || !res.data) return;
      const img = new Image();
      img.onload = () => drawCapeFront(node, img);
      img.src = `data:image/png;base64,${res.data}`;
    })();
  }

  async function pickCape(id: string | null) {
    if (busy) return;
    busy = true;
    saveError = null;
    const res =
      id === null
        ? await commands.clearActiveCape(account.id)
        : await commands.setActiveCape(account.id, id);
    if (res.status === 'error') {
      saveError = $t('cosmetics.saveError');
    } else {
      capes = capes.map((c) => ({ ...c, is_active: c.id === id }));
      void applyCapeToViewer();
    }
    busy = false;
  }

  function chooseSkinFile() {
    if (busy) return;
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = 'image/png';
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      const buf = new Uint8Array(await file.arrayBuffer());
      let binary = '';
      for (const b of buf) binary += String.fromCharCode(b);
      const b64 = btoa(binary);
      busy = true;
      saveError = null;
      const res = await commands.uploadSkin(account.id, b64, variant);
      if (res.status === 'error') {
        saveError = $t('cosmetics.invalidImage');
      } else {
        currentSkinB64 = b64;
        undoSkin = null;
        syncSkinPreview();
      }
      busy = false;
    };
    input.click();
  }

  function requestReset() {
    if (busy) return;
    saveError = null;
    confirmingReset = true;
  }

  function cancelReset() {
    confirmingReset = false;
  }

  async function confirmReset() {
    if (busy) return;
    // Snapshot the current skin BEFORE the reset so it can be restored.
    const snapshot = currentSkinB64 !== null ? { b64: currentSkinB64, variant } : null;
    busy = true;
    saveError = null;
    const res = await commands.resetSkin(account.id);
    busy = false;
    if (res.status === 'error') {
      saveError = $t('cosmetics.saveError');
      return;
    }
    confirmingReset = false;
    undoSkin = snapshot;
    await load(false);
  }

  async function restorePreviousSkin() {
    if (busy || !undoSkin) return;
    const snap = undoSkin;
    busy = true;
    saveError = null;
    const res = await commands.uploadSkin(account.id, snap.b64, snap.variant);
    busy = false;
    if (res.status === 'error') {
      saveError = $t('cosmetics.saveError');
      return;
    }
    variant = snap.variant;
    currentSkinB64 = snap.b64;
    syncSkinPreview();
    undoSkin = null;
  }

  function setVariant(v: SkinVariant) {
    variant = v;
    void applySkinToViewer();
  }

  $effect(() => {
    void load();
  });
</script>

<Modal ariaLabelledby="cosmetics-title" {onClose} panelClass="w-[560px] max-w-full p-0 flex flex-col">
  <div class="flex items-center px-5 py-3.5 border-b border-border-subtle">
    <div>
      <h3 id="cosmetics-title" class="font-medium text-primary text-base">{$t('cosmetics.title')}</h3>
      <p class="text-xs text-muted">{account.name}</p>
    </div>
    <button
      type="button"
      class="ml-auto text-secondary hover:text-primary"
      aria-label={$t('common.close')}
      onclick={onClose}
    >
      <Icon name="close" size={18} />
    </button>
  </div>

  <div class="p-5">
    {#if loading}
      <p class="text-sm text-muted">{$t('common.loading')}</p>
    {:else if loadError}
      <div class="flex items-center gap-3">
        <p class="text-sm text-danger">{$t('cosmetics.loadError')}</p>
        <button type="button" class="btn-secondary btn-xs" onclick={() => load()}>{$t('cosmetics.retry')}</button>
      </div>
    {:else}
      <div class="flex items-baseline gap-2.5 mb-3">
        <div class="text-sm font-medium text-primary">{$t('cosmetics.capeHeading')}</div>
        <div class="text-xs text-muted">{$t('cosmetics.capeHint')}</div>
      </div>
      <div class="grid gap-2.5" style="grid-template-columns:repeat(auto-fit,minmax(88px,1fr))">
        {#each capes as cape (cape.id)}
          <button
            type="button"
            class="flex flex-col items-center gap-1.5 rounded-[10px] border p-2 {cape.is_active
              ? 'border-transparent outline outline-2 outline-accent'
              : 'border-border-subtle hover:border-border-emphasis'}"
            onclick={() => pickCape(cape.id)}
            disabled={busy}
          >
            <canvas use:renderCape={cape.texture_url} class="h-[70px]" style="image-rendering:pixelated"></canvas>
            <span class="text-xs text-secondary truncate w-full text-center">{cape.alias ?? cape.id}</span>
          </button>
        {/each}
        <button
          type="button"
          class="flex flex-col items-center gap-1.5 rounded-[10px] border p-2 {noCapeActive
            ? 'border-transparent outline outline-2 outline-accent'
            : 'border-border-subtle hover:border-border-emphasis'}"
          onclick={() => pickCape(null)}
          disabled={busy}
        >
          <span
            class="h-[70px] w-11 border border-dashed border-border-emphasis rounded flex items-center justify-center text-muted"
          >
            <Icon name="close" size={20} />
          </span>
          <span class="text-xs text-secondary">{$t('cosmetics.noCape')}</span>
        </button>
      </div>
      <p class="mt-2.5 text-xs text-secondary">
        {$t('cosmetics.activeCape', { name: activeCapeName ?? $t('cosmetics.noCape') })}
      </p>

      <div class="h-px bg-border-subtle my-[18px]"></div>

      <div class="flex items-baseline gap-2.5 mb-3">
        <div class="text-sm font-medium text-primary">{$t('cosmetics.skinHeading')}</div>
        <div class="text-xs text-muted">{$t('cosmetics.skinHint')}</div>
      </div>
      <div class="flex gap-[18px] items-start flex-wrap">
        <div class="bg-subtle rounded-[10px] px-3 py-3 flex flex-col items-center gap-2">
          <canvas
            use:mountViewer
            class="w-[200px] h-[260px] cursor-grab active:cursor-grabbing"
            aria-label={$t('cosmetics.currentSkin')}
          ></canvas>
          <span class="text-xs text-muted">{$t('cosmetics.dragToRotate')}</span>
        </div>
        <div class="flex-1 min-w-[200px] flex flex-col gap-3.5">
          <button type="button" class="btn-secondary btn-sm self-start" onclick={chooseSkinFile} disabled={busy}>
            {$t('cosmetics.chooseFile')}
          </button>
          <div>
            <div class="text-xs text-muted mb-1.5">{$t('cosmetics.model')}</div>
            <div class="inline-flex border border-border-subtle rounded overflow-hidden">
              <button
                type="button"
                class="px-4 py-1.5 text-sm {variant === 'classic' ? 'bg-accent-soft text-accent' : 'text-secondary'}"
                onclick={() => setVariant('classic')}>{$t('cosmetics.modelClassic')}</button
              >
              <button
                type="button"
                class="px-4 py-1.5 text-sm {variant === 'slim' ? 'bg-accent-soft text-accent' : 'text-secondary'}"
                onclick={() => setVariant('slim')}>{$t('cosmetics.modelSlim')}</button
              >
            </div>
          </div>
          {#if confirmingReset}
            <div class="flex flex-col gap-2 rounded-[10px] border border-border-subtle p-3">
              <p class="text-xs text-secondary">{$t('cosmetics.resetConfirm')}</p>
              <div class="flex gap-2">
                <button type="button" class="btn-secondary btn-xs" onclick={cancelReset} disabled={busy}>
                  {$t('common.cancel')}
                </button>
                <button type="button" class="btn-danger btn-xs" onclick={confirmReset} disabled={busy}>
                  {$t('cosmetics.resetConfirmYes')}
                </button>
              </div>
            </div>
          {:else}
            <button type="button" class="btn-secondary btn-sm self-start" onclick={requestReset} disabled={busy}>
              {$t('cosmetics.resetSkin')}
            </button>
          {/if}
          {#if undoSkin}
            <button
              type="button"
              class="btn-secondary btn-sm self-start flex items-center gap-1.5"
              onclick={restorePreviousSkin}
              disabled={busy}
            >
              <Icon name="refresh" size={14} />
              {$t('cosmetics.restorePrevious')}
            </button>
          {/if}
        </div>
      </div>

      {#if saveError}
        <p class="mt-3 text-sm text-danger">{saveError}</p>
      {/if}
    {/if}
  </div>
</Modal>
