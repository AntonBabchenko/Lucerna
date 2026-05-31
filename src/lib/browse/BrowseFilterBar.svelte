<script lang="ts">
  import LayoutToggle from '$lib/mods/LayoutToggle.svelte';

  // The compact, never-wrapping toolbar: search (flex-1), a Sort select,
  // the "Filters (n)" button that opens the drawer, and the layout
  // toggle. Search and sort use callbacks (not bind) so each browser can
  // keep its own debounce / narrowly-typed sort state.
  type SortOption = { value: string; label: string };

  let {
    searchAriaLabel,
    searchPlaceholder,
    searchTestid,
    sort,
    sortOptions,
    sortTestid,
    activeCount,
    onSearchInput,
    onSortChange,
    onOpenDrawer,
  }: {
    searchAriaLabel: string;
    searchPlaceholder: string;
    searchTestid?: string;
    sort: string;
    sortOptions: SortOption[];
    sortTestid?: string;
    activeCount: number;
    onSearchInput: (value: string) => void;
    onSortChange: (value: string) => void;
    onOpenDrawer: () => void;
  } = $props();
</script>

<div class="flex items-center gap-2 px-3 py-3">
  <input
    type="search"
    class="filter-control flex-1 min-w-[8rem]"
    aria-label={searchAriaLabel}
    placeholder={searchPlaceholder}
    data-testid={searchTestid}
    oninput={(e) => onSearchInput(e.currentTarget.value)}
  />

  <label class="inline-flex items-center gap-1 text-sm text-secondary">
    Sort:
    <select
      class="filter-control filter-control-select"
      value={sort}
      data-testid={sortTestid}
      onchange={(e) => onSortChange(e.currentTarget.value)}
    >
      {#each sortOptions as opt (opt.value)}
        <option value={opt.value}>{opt.label}</option>
      {/each}
    </select>
  </label>

  <button
    type="button"
    class="filter-control inline-flex items-center gap-1.5"
    aria-haspopup="dialog"
    data-testid="browse-filters-button"
    onclick={onOpenDrawer}
  >
    <span aria-hidden="true">⚙</span>
    Filters
    {#if activeCount > 0}
      <span
        class="inline-flex items-center justify-center min-w-[1.25rem] h-5 px-1 rounded-full bg-accent-soft text-accent text-xs font-medium"
      >
        {activeCount}
      </span>
    {/if}
  </button>

  <LayoutToggle />
</div>
