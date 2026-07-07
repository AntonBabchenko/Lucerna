<script lang="ts">
  import { commands, type Screenshot } from '$lib/ipc/bindings';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { t } from '$lib/i18n';
  import { copyToClipboard, saveCopy, reveal, deleteScreenshot } from './actions';

  let {
    shots,
    index = $bindable(0),
    onClose,
    onDeleted,
  }: {
    shots: Screenshot[];
    index: number;
    onClose: () => void;
    onDeleted: (s: Screenshot) => void;
  } = $props();

  let url = $state<string | null>(null);
  const current = $derived(shots[index]);

  $effect(() => {
    url = null;
    const s = current;
    if (!s) return;
    void commands.screenshotPreview(s.instance_id, s.file_name).then((r) => {
      if (r.status === 'ok' && current === s) url = r.data;
    });
  });

  function prev() {
    index = (index - 1 + shots.length) % shots.length;
  }
  function next() {
    index = (index + 1) % shots.length;
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'ArrowLeft') prev();
    else if (e.key === 'ArrowRight') next();
    else if (e.key === 'Escape') onClose();
  }
  async function onDelete() {
    const s = current;
    if (await deleteScreenshot(s)) onDeleted(s);
  }
</script>

<svelte:window onkeydown={onKey} />

<div
  class="fixed inset-0 z-50 flex flex-col items-center justify-center gap-4 bg-black/60 p-6"
  role="dialog"
  aria-modal="true"
>
  <button
    type="button"
    class="btn-icon absolute right-4 top-4 bg-surface/80"
    aria-label={$t('screenshots.galleryClose')}
    onclick={onClose}><Icon name="close" size={20} /></button
  >

  <div class="relative flex max-h-[75vh] w-full max-w-5xl items-center justify-center">
    {#if url}
      <img
        src={url}
        alt={current?.file_name ?? ''}
        class="max-h-[75vh] max-w-full rounded object-contain"
      />
    {:else}
      <Spinner size="md" delayMs={150} />
    {/if}
    {#if shots.length > 1}
      <button
        type="button"
        class="btn-icon absolute left-2 bg-surface/80"
        aria-label={$t('common.previousImage')}
        onclick={prev}><Icon name="chevronLeft" size={22} /></button
      >
      <button
        type="button"
        class="btn-icon absolute right-2 bg-surface/80"
        aria-label={$t('common.nextImage')}
        onclick={next}><Icon name="chevronRight" size={22} /></button
      >
    {/if}
  </div>

  <div
    class="flex items-center gap-3 rounded-full border border-border-subtle bg-surface px-4 py-2 text-secondary"
  >
    <span class="text-sm text-primary">{index + 1} / {shots.length}</span>
    <span class="h-4 w-px bg-border-subtle"></span>
    <button type="button" use:tooltip={$t('screenshots.reveal')} onclick={() => reveal(current)}
      ><Icon name="folderOpen" size={16} /></button
    >
    <button type="button" use:tooltip={$t('screenshots.saveCopy')} onclick={() => saveCopy(current)}
      ><Icon name="download" size={16} /></button
    >
    <button
      type="button"
      use:tooltip={$t('screenshots.copy')}
      onclick={() => copyToClipboard(current)}><Icon name="copy" size={16} /></button
    >
    <button
      type="button"
      class="text-danger"
      use:tooltip={$t('screenshots.delete')}
      onclick={onDelete}><Icon name="trash" size={16} /></button
    >
  </div>
</div>
