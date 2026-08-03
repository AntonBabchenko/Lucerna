<script lang="ts">
  // Presentational: one group's worth of cards. The lightbox lives in
  // ScreenshotBrowser so it can walk the entire collection rather than the
  // handful of shots that happen to share this group's day.
  import type { Screenshot } from '$lib/ipc/bindings';
  import ScreenshotThumb from './ScreenshotThumb.svelte';

  let { shots, onOpen }: { shots: Screenshot[]; onOpen: (s: Screenshot) => void } = $props();
</script>

<div class="grid grid-cols-2 gap-3 p-3 sm:grid-cols-3 lg:grid-cols-4">
  {#each shots as shot (shot.instance_id + '/' + shot.file_name)}
    <ScreenshotThumb {shot} {onOpen} />
  {/each}
</div>
