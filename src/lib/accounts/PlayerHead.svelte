<script lang="ts">
  import { deriveAccountAvatar } from './account-avatar';
  import { loadSkinHead } from './skin-cache';
  import { drawHead } from './skin-head';

  let {
    uuid,
    name,
    size = 20,
  }: {
    uuid: string;
    name: string;
    size?: number;
  } = $props();

  const fallback = $derived(deriveAccountAvatar(name));
  let pngBase64 = $state<string | null>(null);
  let canvas = $state<HTMLCanvasElement | undefined>();

  // Fetch (cache-backed) whenever the uuid changes; ignore a stale resolve
  // if the uuid changed mid-flight.
  $effect(() => {
    const currentUuid = uuid;
    let cancelled = false;
    pngBase64 = null;
    loadSkinHead(currentUuid).then((b64) => {
      if (!cancelled) pngBase64 = b64;
    });
    return () => {
      cancelled = true;
    };
  });

  // Paint the head once both the PNG and the canvas element exist.
  $effect(() => {
    if (pngBase64 && canvas) drawHead(canvas, pngBase64, size);
  });
</script>

{#if pngBase64}
  <canvas
    bind:this={canvas}
    class="flex-shrink-0 rounded-sm"
    style="width: {size}px; height: {size}px;"
    aria-hidden="true"
  ></canvas>
{:else}
  <span
    class="flex-shrink-0 inline-flex items-center justify-center rounded-sm font-semibold text-white"
    style="width: {size}px; height: {size}px; font-size: {Math.round(
      size * 0.55,
    )}px; background: hsl({fallback.hue} 55% 45%);"
    aria-hidden="true"
  >
    {fallback.letter}
  </span>
{/if}
