<script lang="ts">
  import { commands, type Screenshot } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Select from '$lib/ui/Select.svelte';
  import ScreenshotGrid from './ScreenshotGrid.svelte';

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

  // Day bucket label: Today / Yesterday / locale date.
  function dayLabel(ms: number): string {
    const d = new Date(ms);
    const now = new Date();
    const startOf = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
    const diffDays = Math.round((startOf(now) - startOf(d)) / 86_400_000);
    if (diffDays === 0) return $t('screenshots.groupToday');
    if (diffDays === 1) return $t('screenshots.groupYesterday');
    return d.toLocaleDateString();
  }

  // Group the filtered shots (already newest-first from the backend) by day.
  const groups = $derived.by(() => {
    const map = new Map<string, Screenshot[]>();
    for (const s of filtered) {
      const key = dayLabel(s.modified_unix_ms ?? 0);
      const arr = map.get(key);
      if (arr) arr.push(s);
      else map.set(key, [s]);
    }
    return [...map.entries()];
  });
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
    {#if all.length > 0}
      <Select
        class="w-56 text-sm"
        value={instanceFilter}
        options={instanceOptions}
        onChange={(v) => (instanceFilter = String(v))}
        ariaLabel={$t('screenshots.filterInstance')}
      />
    {/if}
    <button
      type="button"
      class="btn-icon"
      aria-label={$t('screenshots.galleryClose')}
      onclick={onClose}
    >
      <Icon name="close" size={20} />
    </button>
  </header>

  <div class="min-h-0 flex-1 overflow-y-auto">
    {#if loading}
      <LoadingPanel label={$t('screenshots.loading')} />
    {:else if listError}
      <p class="p-4 text-sm text-danger">{listError}</p>
    {:else if filtered.length === 0}
      <p class="p-8 text-center text-sm text-muted">{$t('screenshots.emptyGallery')}</p>
    {:else}
      {#each groups as [label, shots] (label)}
        <h3 class="px-4 pt-4 text-sm font-medium text-secondary">{label}</h3>
        <ScreenshotGrid {shots} onChanged={reload} />
      {/each}
    {/if}
  </div>
</Modal>
