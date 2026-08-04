<script lang="ts">
  // The single owner of screenshot navigation: ordering, grouping, how much of
  // the list is rendered, and the lightbox. Both the instance tab and the
  // gallery mount this, so the two surfaces cannot drift apart.
  import type { Snippet } from 'svelte';
  import { locale, t } from '$lib/i18n';
  import type { Screenshot } from '$lib/ipc/bindings';
  import { screenshotGranularity, screenshotSortDir } from '$lib/settings/state.svelte';
  import SegmentedControl from '$lib/ui/SegmentedControl.svelte';
  import ScreenshotGrid from './ScreenshotGrid.svelte';
  import ScreenshotLightbox from './ScreenshotLightbox.svelte';
  import {
    type Granularity,
    type SortDir,
    groupLabel,
    groupShots,
    sortShots,
  } from './screenshots-view';

  // Divisible by 2, 3 and 4 — whole rows at every breakpoint of the grid.
  const PAGE_SIZE = 120;

  let {
    shots,
    onChanged = () => {},
    controls,
    resetKey,
  }: {
    shots: Screenshot[];
    onChanged?: () => void;
    controls?: Snippet;
    resetKey: string;
  } = $props();

  let visible = $state(PAGE_SIZE);
  let lightboxIndex = $state<number | null>(null);

  // Order, THEN slice, THEN group. Grouping first and taking N groups would
  // pull a whole month at once — exactly what month granularity produces.
  const ordered = $derived(sortShots(shots, screenshotSortDir.value));
  const shown = $derived(ordered.slice(0, visible));
  const groups = $derived(groupShots(shown, screenshotGranularity.value));

  // A different collection, or a different end of it, starts from one page.
  // Granularity is deliberately absent: it only changes the headers, and a
  // reload after a delete must not collapse the pages the user revealed.
  $effect(() => {
    void resetKey;
    void screenshotSortDir.value;
    visible = PAGE_SIZE;
  });

  function open(shot: Screenshot) {
    lightboxIndex = ordered.findIndex(
      (s) => s.instance_id === shot.instance_id && s.file_name === shot.file_name,
    );
  }

  function closeLightbox() {
    // Keep the user's place: paging past the rendered window inside the
    // lightbox would otherwise drop them back into a grid that no longer
    // contains where they were.
    if (lightboxIndex !== null && lightboxIndex >= visible) {
      visible = Math.ceil((lightboxIndex + 1) / PAGE_SIZE) * PAGE_SIZE;
    }
    lightboxIndex = null;
  }

  const granularityOptions = $derived([
    { value: 'day', label: $t('screenshots.groupDays'), testId: 'shots-granularity-day' },
    { value: 'month', label: $t('screenshots.groupMonths'), testId: 'shots-granularity-month' },
  ]);

  const sortOptions = $derived([
    { value: 'newest', label: $t('screenshots.sortNewest'), testId: 'shots-sort-newest' },
    { value: 'oldest', label: $t('screenshots.sortOldest'), testId: 'shots-sort-oldest' },
  ]);
</script>

<div class="flex flex-wrap items-center gap-3 px-3 pt-3">
  <SegmentedControl
    variant="boxed"
    ariaLabel={$t('screenshots.groupBy')}
    options={granularityOptions}
    value={screenshotGranularity.value}
    onChange={(v) => (screenshotGranularity.value = v as Granularity)}
  />
  <SegmentedControl
    variant="boxed"
    ariaLabel={$t('screenshots.sortBy')}
    options={sortOptions}
    value={screenshotSortDir.value}
    onChange={(v) => (screenshotSortDir.value = v as SortDir)}
  />
  <span class="flex-1"></span>
  {@render controls?.()}
</div>

{#each groups as group (group.key)}
  <h3
    class="sticky top-0 z-10 border-b border-border-subtle bg-subtle px-4 py-1 text-xs font-semibold text-muted"
  >
    {groupLabel($t, $locale ?? 'en', group.startMs, screenshotGranularity.value)}
  </h3>
  <ScreenshotGrid shots={group.shots} onOpen={open} />
{/each}

<div class="flex items-center justify-center gap-3 p-4 text-sm text-muted">
  {#if visible < ordered.length}
    <span data-testid="shots-count">
      {$t('screenshots.shownOf', { shown: shown.length, total: ordered.length })}
    </span>
    <button
      type="button"
      class="btn-secondary btn-sm"
      data-testid="shots-show-more"
      onclick={() => (visible += PAGE_SIZE)}
    >
      {$t('screenshots.showMore')}
    </button>
  {:else}
    <span data-testid="shots-count">{$t('screenshots.total', { n: ordered.length })}</span>
  {/if}
</div>

{#if lightboxIndex !== null}
  <ScreenshotLightbox
    shots={ordered}
    bind:index={lightboxIndex}
    onClose={closeLightbox}
    {onChanged}
    onDeleted={() => {
      lightboxIndex = null;
      onChanged();
    }}
  />
{/if}
