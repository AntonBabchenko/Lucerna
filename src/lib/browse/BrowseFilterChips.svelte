<script lang="ts">
  import { t } from '$lib/i18n';
  import type { FilterChip, FilterChipKey } from './filter-model';

  // Applied-filter chip row, shown above the result grid. Each chip is a
  // button that removes its facet; "Clear all" resets everything. The
  // whole row is absent when no facet is active. `clearAllTestid` lets
  // each browser keep its historical test id (mod-clear-filters /
  // modpack-clear-filters).
  let {
    chips,
    onClear,
    onClearAll,
    clearAllTestid,
  }: {
    chips: FilterChip[];
    onClear: (key: FilterChipKey) => void;
    onClearAll: () => void;
    clearAllTestid: string;
  } = $props();
</script>

{#if chips.length > 0}
  <div class="flex flex-wrap items-center gap-2 px-3 pb-2" data-testid="browse-filter-chips">
    {#each chips as chip (chip.key)}
      <button
        type="button"
        class="inline-flex items-center gap-1 rounded-full border border-accent/40 bg-accent/15 px-2 py-0.5 text-xs text-accent"
        data-testid={`browse-chip-${chip.key}`}
        onclick={() => onClear(chip.key)}
      >
        <span>{chip.label}</span>
        <span aria-hidden="true">✕</span>
        <span class="sr-only">{$t('browse.filter.removeFilter')}</span>
      </button>
    {/each}
    <button
      type="button"
      class="btn-tertiary text-xs"
      data-testid={clearAllTestid}
      onclick={onClearAll}
    >
      {$t('browse.filter.clearAll')}
    </button>
  </div>
{/if}
