<script lang="ts">
  import { commands, type Screenshot } from '$lib/ipc/bindings';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { tooltip } from '$lib/ui/tooltip';

  let { shot, onOpen }: { shot: Screenshot; onOpen: (s: Screenshot) => void } = $props();

  let url = $state<string | null>(null);
  let el = $state<HTMLButtonElement | null>(null);

  $effect(() => {
    if (!el || url) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          io.disconnect();
          void load();
        }
      },
      { rootMargin: '200px' },
    );
    io.observe(el);
    return () => io.disconnect();
  });

  async function load() {
    const r = await commands.screenshotThumbnail(shot.instance_id, shot.file_name);
    if (r.status === 'ok') url = r.data;
  }
</script>

<button
  bind:this={el}
  type="button"
  data-testid="screenshot-card"
  class="relative block aspect-video w-full overflow-hidden rounded border border-border-subtle bg-base"
  onclick={() => onOpen(shot)}
  use:tooltip={shot.file_name}
>
  {#if url}
    <img src={url} alt={shot.file_name} class="h-full w-full object-cover" />
  {:else}
    <span class="absolute inset-0 flex items-center justify-center">
      <Spinner size="sm" delayMs={150} />
    </span>
  {/if}
</button>
