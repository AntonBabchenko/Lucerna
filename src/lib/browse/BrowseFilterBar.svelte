<script lang="ts">
  import { t } from '$lib/i18n';
  import type { LoaderKind } from '$lib/ipc/bindings';
  import LayoutToggle from '$lib/mods/LayoutToggle.svelte';
  import McVersionCombobox from '$lib/mods/McVersionCombobox.svelte';
  import Select from '$lib/ui/Select.svelte';

  // The browse toolbar. All facet controls live inline next to Sort — no
  // drawer, no chip row: the user complained the drawer made filters feel
  // separated and hard to reach. Loader renders as a compact themed dropdown,
  // MC as the shared combobox, Show-installed as a checkbox, and a
  // "Match instance" / "Clear all" pair appears only when relevant. Source is a
  // context switch, not a filter, so it lives in the sub-tab header row
  // (SourcePicker), not here.
  //
  // Facet values are bound (loader/mc) so the parent's existing search effect
  // re-runs on change. Search + sort stay callbacks so each browser keeps its
  // own debounce / narrowly-typed sort state. Optional facets render only when
  // their props are supplied: loader = mod browser (mods only), showInstalled =
  // mod browser.
  type SortOption = { value: string; label: string };

  let {
    searchAriaLabel,
    searchPlaceholder,
    searchTestid,
    value = '',
    sort,
    sortOptions,
    sortTestid,
    onSearchInput,
    onSortChange,
    showLoader = false,
    loader = $bindable<LoaderKind | ''>(''),
    mc = $bindable(''),
    mcTestid,
    showInstalled = undefined,
    onShowInstalledChange,
    serverFilters = true,
    serverFilterNote = undefined,
    canRestore = false,
    restoreLabel = undefined,
    onRestore = undefined,
    activeCount = 0,
    onClearAll = undefined,
  }: {
    searchAriaLabel: string;
    searchPlaceholder: string;
    searchTestid?: string;
    // The current query, owned by the parent. Supplied so a query set
    // PROGRAMMATICALLY is visible: the compat panel hands a missing dependency
    // over to Browse and seeds the search with its mod-id. Without this the
    // request goes out while the box stays blank, and "nothing found" under an
    // empty field reads as a broken search rather than an honest miss.
    //
    // One-way, not `bind:` — the parent sets its own state synchronously in
    // `onSearchInput` before any debounce, so typing never lags and the caret
    // never jumps.
    value?: string;
    sort: string;
    sortOptions: SortOption[];
    sortTestid?: string;
    onSearchInput: (value: string) => void;
    onSortChange: (value: string) => void;
    showLoader?: boolean;
    loader?: LoaderKind | '';
    mc?: string;
    mcTestid?: string;
    // showInstalled is a controlled value + callback (not bound) because the
    // mod browser needs bespoke handling when it flips.
    showInstalled?: boolean | undefined;
    onShowInstalledChange?: (value: boolean) => void;
    // When false (FTB), loader + MC are server-unsupported → greyed out.
    serverFilters?: boolean;
    serverFilterNote?: string | undefined;
    canRestore?: boolean;
    restoreLabel?: string | undefined;
    onRestore?: (() => void) | undefined;
    // activeCount > 0 surfaces a "Clear all" button wired to onClearAll.
    activeCount?: number;
    onClearAll?: (() => void) | undefined;
  } = $props();

  const LOADER_OPTIONS = $derived([
    { value: '', label: $t('browse.filter.any') },
    { value: 'fabric', label: 'Fabric' },
    { value: 'quilt', label: 'Quilt' },
    { value: 'forge', label: 'Forge' },
    { value: 'neoforge', label: 'NeoForge' },
  ]);
</script>

<div class="flex flex-wrap items-center gap-2 px-3 py-3">
  <input
    type="search"
    class="filter-control flex-1 min-w-[8rem]"
    aria-label={searchAriaLabel}
    placeholder={searchPlaceholder}
    data-testid={searchTestid}
    {value}
    oninput={(e) => onSearchInput(e.currentTarget.value)}
  />

  {#if showLoader}
    <label
      class="inline-flex items-center gap-1 text-sm text-secondary"
      class:opacity-50={!serverFilters}
    >
      {$t('browse.filter.loaderShort')}
      <Select
        class="filter-control filter-control-select"
        value={loader}
        options={LOADER_OPTIONS}
        disabled={!serverFilters}
        onChange={(v) => (loader = v as LoaderKind | '')}
        dataTestid="browse-loader-select"
      />
    </label>
  {/if}

  <label
    class="inline-flex items-center gap-1 text-sm text-secondary"
    class:opacity-50={!serverFilters}
  >
    {$t('browse.filter.mcShort')}
    <McVersionCombobox
      bind:value={mc}
      dataTestid={mcTestid}
      placeholder={$t('browse.filter.any')}
      disabled={!serverFilters}
    />
  </label>

  <label class="inline-flex items-center gap-1 text-sm text-secondary">
    {$t('browse.filter.sortLabel')}
    <Select
      class="filter-control filter-control-select"
      value={sort}
      options={sortOptions}
      onChange={(v) => onSortChange(String(v))}
      dataTestid={sortTestid}
    />
  </label>

  {#if showInstalled !== undefined && onShowInstalledChange}
    <label class="inline-flex items-center gap-1.5 text-sm text-secondary whitespace-nowrap">
      <input
        type="checkbox"
        checked={showInstalled}
        data-testid="browse-show-installed"
        onchange={(e) => onShowInstalledChange(e.currentTarget.checked)}
      />
      {$t('browse.filter.showInstalled')}
    </label>
  {/if}

  {#if canRestore && onRestore}
    <button
      type="button"
      class="btn-tertiary text-xs"
      data-testid="browse-restore-instance"
      onclick={onRestore}
    >
      {restoreLabel ?? $t('browse.filter.restoreForInstance')}
    </button>
  {/if}

  {#if activeCount > 0 && onClearAll}
    <button
      type="button"
      class="btn-tertiary text-xs"
      data-testid="browse-clear-filters"
      onclick={onClearAll}
    >
      {$t('browse.filter.clearAll')}
    </button>
  {/if}

  {#if !serverFilters && serverFilterNote}
    <span class="w-full text-xs text-placeholder">{serverFilterNote}</span>
  {/if}

  <LayoutToggle />
</div>
