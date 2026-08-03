<script lang="ts">
  import { t } from '$lib/i18n';
  import { commands, type Screenshot } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Select from '$lib/ui/Select.svelte';
  import ScreenshotBrowser from './ScreenshotBrowser.svelte';

  let { onClose }: { onClose: () => void } = $props();

  let all = $state<Screenshot[]>([]);
  let loading = $state(false);
  let listError = $state<string | null>(null);
  let instanceFilter = $state(''); // '' = all instances

  async function reload() {
    loading = true;
    listError = null;
    const r = await commands.listAllScreenshots();
    loading = false;
    if (r.status === 'ok') all = r.data;
    else listError = formatError(r.error);
  }
  $effect(() => {
    void reload();
  });

  // One option per instance that actually has screenshots, plus "all".
  const instanceOptions = $derived([
    { value: '', label: $t('screenshots.filterAll') },
    ...[...new Map(all.map((s) => [s.instance_id, s.instance_name])).entries()].map(
      ([value, label]) => ({ value, label }),
    ),
  ]);

  const filtered = $derived(
    instanceFilter ? all.filter((s) => s.instance_id === instanceFilter) : all,
  );
</script>

<Modal
  ariaLabelledby="screenshots-gallery-title"
  {onClose}
  panelClass="w-[92vw] max-w-6xl h-[92vh] flex flex-col"
>
  <header class="flex shrink-0 items-center gap-3 border-b border-border-subtle p-4">
    <Icon name="gallery" size={18} />
    <h2 id="screenshots-gallery-title" class="font-semibold text-primary">
      {$t('screenshots.galleryTitle')}
    </h2>
    <span class="flex-1"></span>
    <CloseButton ariaLabel={$t('screenshots.galleryClose')} onClick={onClose} />
  </header>

  <div class="min-h-0 flex-1 overflow-y-auto">
    {#if loading}
      <LoadingPanel label={$t('screenshots.loading')} />
    {:else if listError}
      <p class="p-4 text-sm text-danger" role="alert">{listError}</p>
    {:else if filtered.length === 0}
      <p class="p-8 text-center text-sm text-muted">{$t('screenshots.emptyGallery')}</p>
    {:else}
      <ScreenshotBrowser
        shots={filtered}
        onChanged={reload}
        resetKey={instanceFilter}
        {controls}
      />
    {/if}
  </div>
</Modal>

{#snippet controls()}
  {#if all.length > 0}
    <Select
      class="w-56 text-sm"
      value={instanceFilter}
      options={instanceOptions}
      onChange={(v) => (instanceFilter = String(v))}
      ariaLabel={$t('screenshots.filterInstance')}
    />
  {/if}
{/snippet}
