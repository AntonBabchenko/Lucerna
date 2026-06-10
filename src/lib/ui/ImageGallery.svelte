<script lang="ts">
  import type { GalleryImage } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';

  // Compact screenshot carousel for the detail modals. Single image in
  // view; prev/next arrows step through. Renders nothing when there are no images.
  let { images }: { images: GalleryImage[] } = $props();

  let index = $state(0);

  // Clamp when the image set changes (e.g. modal reused for another mod).
  $effect(() => {
    if (index > images.length - 1) index = 0;
  });

  function prev() {
    index = (index - 1 + images.length) % images.length;
  }
  function next() {
    index = (index + 1) % images.length;
  }
</script>

{#if images.length > 0}
  <div class="relative rounded overflow-hidden bg-base border border-border-subtle">
    <img
      src={images[index].url}
      alt={images[index].title ?? ''}
      loading="lazy"
      class="w-full max-h-72 object-contain bg-black/5"
    />
    {#if images.length > 1}
      <button
        type="button"
        aria-label={$t('common.previousImage')}
        class="absolute left-1 top-1/2 -translate-y-1/2 bg-surface/80 rounded-full w-7 h-7 flex items-center justify-center"
        onclick={prev}><Icon name="chevronLeft" size={18} /></button
      >
      <button
        type="button"
        aria-label={$t('common.nextImage')}
        class="absolute right-1 top-1/2 -translate-y-1/2 bg-surface/80 rounded-full w-7 h-7 flex items-center justify-center"
        onclick={next}><Icon name="chevronRight" size={18} /></button
      >
      <div
        class="absolute bottom-1 right-2 text-xs bg-surface/80 rounded px-1.5 py-0.5 text-secondary"
      >
        {index + 1} / {images.length}
      </div>
    {/if}
    {#if images[index].title}
      <div
        class="absolute bottom-1 left-2 text-xs bg-surface/80 rounded px-1.5 py-0.5 text-secondary max-w-[60%] truncate"
      >
        {images[index].title}
      </div>
    {/if}
  </div>
{/if}
