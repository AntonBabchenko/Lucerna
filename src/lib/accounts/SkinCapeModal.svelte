<script lang="ts">
  import Modal from '$lib/ui/Modal.svelte';
  import { Icon } from '$lib/ui/icons';
  import { t } from '$lib/i18n';
  import { commands, type Account, type CapeInfo, type SkinVariant } from '$lib/ipc/bindings';
  import { drawCapeFront } from '$lib/accounts/cape-render';
  import { drawSkinFront } from '$lib/accounts/skin-render';

  let { account, onClose }: { account: Account; onClose: () => void } = $props();

  let loading = $state(true);
  let loadError = $state(false);
  let saveError = $state<string | null>(null);
  let busy = $state(false);
  let capes = $state<CapeInfo[]>([]);
  let variant = $state<SkinVariant>('classic');
  let skinImg: HTMLImageElement | null = null;
  let skinCanvas = $state<HTMLCanvasElement | null>(null);
  // Base64 of the currently-displayed skin, kept so a reset can snapshot it.
  let currentSkinB64: string | null = null;
  // Two-step guard for the destructive reset, plus an in-session undo snapshot
  // of the skin that was active before the reset (so it can be re-applied).
  let confirmingReset = $state(false);
  let undoSkin = $state<{ b64: string; variant: SkinVariant } | null>(null);

  const activeCape = $derived(capes.find((c) => c.is_active) ?? null);
  const activeCapeName = $derived(activeCape ? (activeCape.alias ?? activeCape.id) : null);
  const noCapeActive = $derived(activeCape === null);

  async function refreshSkin() {
    const skinRes = await commands.accountSkin(account.uuid);
    if (skinRes.status === 'ok' && skinRes.data) {
      currentSkinB64 = skinRes.data.skin_png_base64;
      const img = new Image();
      img.onload = () => {
        skinImg = img;
        renderSkin();
      };
      img.src = `data:image/png;base64,${skinRes.data.skin_png_base64}`;
    } else {
      currentSkinB64 = null;
      skinImg = null;
      const ctx = skinCanvas?.getContext('2d');
      if (skinCanvas && ctx) ctx.clearRect(0, 0, skinCanvas.width, skinCanvas.height);
    }
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

  function renderSkin() {
    if (skinCanvas && skinImg) drawSkinFront(skinCanvas, skinImg, variant);
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
        const img = new Image();
        img.onload = () => {
          skinImg = img;
          renderSkin();
        };
        img.src = `data:image/png;base64,${b64}`;
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
    const img = new Image();
    img.onload = () => {
      skinImg = img;
      renderSkin();
    };
    img.src = `data:image/png;base64,${snap.b64}`;
    undoSkin = null;
  }

  function setVariant(v: SkinVariant) {
    variant = v;
    renderSkin();
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
        <div class="bg-subtle rounded-[10px] px-5 py-3 flex flex-col items-center gap-2">
          <canvas bind:this={skinCanvas} class="h-[150px]" style="image-rendering:pixelated"></canvas>
          <span class="text-xs text-muted">{$t('cosmetics.currentSkin')}</span>
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
