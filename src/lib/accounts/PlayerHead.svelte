<!--
  PlayerHead — an account's Minecraft skin head (canvas), or a deterministic
  letter avatar when no skin is available. Decorative by design: it is always
  rendered next to the account's name/label, so both branches are aria-hidden.
  Do not use it as the sole identifier of an account with no adjacent text.
-->
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

  // Paint the head once both the PNG and the canvas element exist. drawHead
  // returns a dispose that aborts a pending decode on teardown / re-run. If the
  // PNG fails to decode, clear it so the letter fallback renders instead of a
  // blank canvas.
  $effect(() => {
    if (!pngBase64 || !canvas) return;
    return drawHead(canvas, pngBase64, size, () => {
      pngBase64 = null;
    });
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
