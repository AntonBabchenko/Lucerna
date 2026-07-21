<script lang="ts">
  import { commands, type Screenshot } from '$lib/ipc/bindings';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import Modal from '$lib/ui/Modal.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { t } from '$lib/i18n';
  import { copyToClipboard, saveAnnotated, reveal, deleteScreenshot } from './actions';
  import ScreenshotAnnotator from './ScreenshotAnnotator.svelte';

  let {
    shots,
    index = $bindable(0),
    onClose,
    onChanged = () => {},
    onDeleted,
  }: {
    shots: Screenshot[];
    index: number;
    onClose: () => void;
    onChanged?: () => void;
    onDeleted: (s: Screenshot) => void;
  } = $props();

  let url = $state<string | null>(null);
  const current = $derived(shots[index]);

  // Annotator instance (remounted per image) — exposes the strokes overlay and
  // applied crop for the "save annotated copy" flow.
  let annotator = $state<
    | {
        overlayDataUrl(): string | null;
        cropRect(): { x: number; y: number; w: number; h: number } | null;
      }
    | undefined
  >();

  // A save writes a new file into the screenshots folder; refresh the list once
  // the lightbox closes (reloading while open would shift indices and jump to a
  // different image).
  let saved = $state(false);

  async function saveCurrent() {
    if (!annotator) return;
    if (await saveAnnotated(current, annotator.overlayDataUrl(), annotator.cropRect())) {
      saved = true;
    }
  }

  function close() {
    onClose();
    if (saved) onChanged();
  }

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
  // Escape is owned by Modal's topmost-only open-stack (so closing the
  // lightbox never also closes the gallery modal under it); this window
  // handler only adds the arrow-key navigation.
  function onKey(e: KeyboardEvent) {
    if (e.key === 'ArrowLeft') prev();
    else if (e.key === 'ArrowRight') next();
  }
  async function onDelete() {
    const s = current;
    if (await deleteScreenshot(s)) onDeleted(s);
  }
</script>

<svelte:window onkeydown={onKey} />

<Modal
  onClose={close}
  bare
  ariaLabel={$t('screenshots.viewerTitle')}
  panelClass="relative flex flex-col items-center justify-center gap-4 p-6"
>
  <!-- Full-bleed backdrop: a real <button>, so click + Enter/Space close it
       (Esc via Modal) with zero a11y warnings. It sits behind the image and
       controls, which are lifted above it with z-10 — so a click on any dark
       area closes, while clicks on the image / arrows / toolbar hit those
       instead. -->
  <button
    type="button"
    class="absolute inset-0 cursor-default"
    aria-label={$t('screenshots.galleryClose')}
    onclick={close}
  ></button>

  <button
    type="button"
    class="btn-icon absolute right-4 top-4 z-10 bg-surface/80"
    aria-label={$t('screenshots.galleryClose')}
    use:tooltip={$t('screenshots.galleryClose')}
    onclick={close}><Icon name="close" size={20} /></button
  >

  {#if url}
    {#key current?.file_name}
      <ScreenshotAnnotator {url} bind:this={annotator} />
    {/key}
  {:else}
    <div class="relative z-10 flex items-center justify-center">
      <Spinner size="md" delayMs={150} />
    </div>
  {/if}

  <div
    class="relative z-10 flex items-center gap-3 rounded-full border border-border-subtle bg-surface px-4 py-2 text-secondary"
  >
    {#if shots.length > 1}
      <button
        type="button"
        class="btn-icon btn-icon-sm"
        aria-label={$t('common.previousImage')}
        use:tooltip={$t('common.previousImage')}
        onclick={prev}><Icon name="chevronLeft" size={16} /></button
      >
      <span class="min-w-[3.5rem] text-center text-sm text-primary"
        >{index + 1} / {shots.length}</span
      >
      <button
        type="button"
        class="btn-icon btn-icon-sm"
        aria-label={$t('common.nextImage')}
        use:tooltip={$t('common.nextImage')}
        onclick={next}><Icon name="chevronRight" size={16} /></button
      >
      <span class="h-4 w-px bg-border-subtle"></span>
    {/if}
    <button
      type="button"
      class="btn-icon btn-icon-sm"
      aria-label={$t('screenshots.reveal')}
      use:tooltip={$t('screenshots.reveal')}
      onclick={() => reveal(current)}><Icon name="folderOpen" size={16} /></button
    >
    <button
      type="button"
      class="btn-icon btn-icon-sm"
      aria-label={$t('screenshots.saveCopy')}
      use:tooltip={$t('screenshots.saveCopy')}
      onclick={saveCurrent}><Icon name="download" size={16} /></button
    >
    <button
      type="button"
      class="btn-icon btn-icon-sm"
      aria-label={$t('screenshots.copy')}
      use:tooltip={$t('screenshots.copy')}
      onclick={() => copyToClipboard(current)}><Icon name="copy" size={16} /></button
    >
    <button
      type="button"
      class="btn-icon btn-icon-sm btn-icon-danger"
      aria-label={$t('screenshots.delete')}
      use:tooltip={$t('screenshots.delete')}
      onclick={onDelete}><Icon name="trash" size={16} /></button
    >
  </div>
</Modal>
