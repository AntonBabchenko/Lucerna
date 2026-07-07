<script lang="ts">
  import type { Screenshot } from '$lib/ipc/bindings';
  import ScreenshotThumb from './ScreenshotThumb.svelte';
  import ScreenshotLightbox from './ScreenshotLightbox.svelte';

  let { shots, onChanged = () => {} }: { shots: Screenshot[]; onChanged?: () => void } = $props();

  let lightboxIndex = $state<number | null>(null);

  function open(s: Screenshot) {
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
    onDeleted={() => {
      lightboxIndex = null;
      onChanged();
    }}
  />
{/if}
