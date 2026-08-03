<script lang="ts">
  import type { Screenshot } from '$lib/ipc/bindings';
  import ScreenshotThumb from './ScreenshotThumb.svelte';
  import ScreenshotLightbox from './ScreenshotLightbox.svelte';

  // `onOpen` lets a parent own the lightbox across several grids. Without it
  // the grid falls back to its own lightbox, which can only walk the shots of
  // this one grid — the legacy path, removed once both surfaces migrate.
  let {
    shots,
    onChanged = () => {},
    onOpen,
  }: {
    shots: Screenshot[];
    onChanged?: () => void;
    onOpen?: (s: Screenshot) => void;
  } = $props();

  let lightboxIndex = $state<number | null>(null);

  function open(s: Screenshot) {
    if (onOpen) {
      onOpen(s);
      return;
    }
    lightboxIndex = shots.findIndex(
      (x) => x.instance_id === s.instance_id && x.file_name === s.file_name,
    );
  }
</script>

<div class="grid grid-cols-2 gap-3 p-3 sm:grid-cols-3 lg:grid-cols-4">
  {#each shots as shot (shot.instance_id + '/' + shot.file_name)}
    <ScreenshotThumb {shot} onOpen={open} />
  {/each}
</div>

{#if lightboxIndex !== null}
  <ScreenshotLightbox
    {shots}
    bind:index={lightboxIndex}
    onClose={() => (lightboxIndex = null)}
    {onChanged}
    onDeleted={() => {
      lightboxIndex = null;
      onChanged();
    }}
  />
{/if}
