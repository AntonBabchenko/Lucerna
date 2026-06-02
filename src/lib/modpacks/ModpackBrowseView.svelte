<script lang="ts">
  import { commands } from '$lib/ipc/bindings';
  import type {
    LoaderKind,
    ModpackHit,
    ModpackSearchPage,
    ModpackSort,
    ModSource,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { cfKeyVersion, settingsOpen } from '$lib/settings/state.svelte';
  import { browserPrefs } from '$lib/mods/browser-prefs.svelte';
  import CurseForgeKeyBanner from '$lib/mods/CurseForgeKeyBanner.svelte';
  import PageSizePicker from '$lib/mods/PageSizePicker.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { prioritizeByTitle } from '$lib/mods/search-rank';
  import BrowseFilterBar from '$lib/browse/BrowseFilterBar.svelte';
  import BrowseFilterChips from '$lib/browse/BrowseFilterChips.svelte';
  import BrowseFilterDrawer from '$lib/browse/BrowseFilterDrawer.svelte';
  import { activeChips, activeCount, type FilterChipKey } from '$lib/browse/filter-model';
  import ModpackCard from './ModpackCard.svelte';
  import { t } from '$lib/i18n';

  // Search + paginated grid of modpack hits backed by `modpack_search`
  // on the Rust side. Mirrors the v0.5.0 sub-3 mod browser structure:
  // debounce on input change, prev/next on page change, the empty-state
  // message when a query returns 0 hits. Picking a card hands the
  // ModpackHit back to the parent (ModpacksTab), which opens the
  // version drawer.

  // Modpacks aren't tied to the selected instance — installing a pack
  // creates a NEW instance. The browser must read as instance-agnostic
  // from the toolbar down, so the filter inputs are NOT pre-filled
  // from the active instance.
  let { onPickHit }: { onPickHit: (hit: ModpackHit, mc: string | null) => void } = $props();

  let query = $state('');
  let source = $state<ModSource>('modrinth');

  // CurseForge needs an API key; Modrinth is anonymous. When the user
  // picks CurseForge with no key stored, the whole search UI is
  // replaced by the key banner — same pattern as the sub-3 mod browser.
  let needsCfKey = $state(false);

  async function refreshCfKey() {
    if (source !== 'curseforge') {
      needsCfKey = false;
      return;
    }
    const wasGated = needsCfKey;
    const s = await commands.modsGetCurseforgeKeyStatus();
    needsCfKey = s.status === 'ok' ? s.data === 'missing' : true;
    // Banner just lifted (the user saved a key in Settings). The search
    // $effect won't re-run on its own — none of its watched filters
    // changed — so kick off a search manually.
    if (wasGated && !needsCfKey) {
      runSearch();
    }
  }

  // Re-poll on source flip and whenever Settings saves/clears a key.
  $effect(() => {
    void source;
    void cfKeyVersion.value;
    void refreshCfKey();
  });

  // Filters start empty — the modpack browser is independent of the
  // selected instance, so we don't make assumptions about what MC /
  // loader the user wants. They pick.
  let mcFilter = $state('');
  let loaderFilter = $state<LoaderKind | ''>('');

  let sortChoice = $state<ModpackSort>('relevance');
  let page = $state<ModpackSearchPage | null>(null);
  let pageNum = $state(0);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let drawerOpen = $state(false);
  let debounce: ReturnType<typeof setTimeout> | null = null;

  // Push hits whose title contains the search query to the top — see
  // prioritizeByTitle for the rationale. Empty page falls back to [].
  const sortedHits = $derived(page ? prioritizeByTitle(page.hits, query, (h) => h.title) : []);

  function runSearch() {
    if (debounce) clearTimeout(debounce);
    debounce = setTimeout(async () => {
      // refreshCfKey is async; on a source flip the debounce may fire
      // before its IPC resolves. This guard is the fast path — the
      // mods_platform_auth fallback below is the safety net if
      // needsCfKey is still stale-false.
      if (source === 'curseforge' && needsCfKey) {
        page = null;
        return;
      }
      loading = true;
      error = null;
      try {
        const mc = mcFilter.trim() || null;
        const loaderArg = loaderFilter ? (loaderFilter as LoaderKind) : null;
        const result = await commands.modpackSearch(
          source,
          query,
          pageNum,
          mc,
          loaderArg,
          sortChoice,
          browserPrefs.pageSize,
        );
        if (result.status === 'ok') {
          page = result.data;
        } else if (result.error.kind === 'mods_platform_auth') {
          needsCfKey = true;
          page = null;
        } else {
          error = formatError(result.error);
        }
      } catch (e) {
        error = String(e);
      } finally {
        loading = false;
      }
    }, 300);
  }

  // Modpack browser has no "show installed" facet, so it never enters
  // the chip model. source is a context switch, not a chip (see
  // filter-model). Only loader + mc surface as chips here.
  const filterFacets = $derived({ loader: loaderFilter, mc: mcFilter });

  function clearChip(key: FilterChipKey) {
    if (key === 'loader') loaderFilter = '';
    else if (key === 'mc') mcFilter = '';
  }

  function clearAllFilters() {
    loaderFilter = '';
    mcFilter = '';
  }

  // Re-run search on any reactive input change.
  $effect(() => {
    void source;
    void query;
    void mcFilter;
    void loaderFilter;
    void sortChoice;
    void pageNum;
    void browserPrefs.pageSize;
    runSearch();
  });

  // Reset paginator when filters (NOT pageNum itself) change — otherwise
  // a narrowed query could land the user on an empty page mid-list.
  let prevFilters = $state('');
  $effect(() => {
    const fp = `${source}|${query}|${mcFilter}|${loaderFilter}|${sortChoice}|${browserPrefs.pageSize}`;
    if (fp !== prevFilters) {
      prevFilters = fp;
      if (pageNum !== 0) pageNum = 0;
    }
  });
</script>

<div data-tour-ctx="modpacks-filters" class="pt-2">
  <BrowseFilterBar
    searchAriaLabel={$t('modpacks.browse.searchAriaLabel')}
    searchPlaceholder={source === 'curseforge'
      ? $t('modpacks.browse.searchPlaceholderCurseForge')
      : $t('modpacks.browse.searchPlaceholderModrinth')}
    searchTestid="modpack-search-input"
    sort={sortChoice}
    sortOptions={[
      { value: 'relevance', label: $t('modpacks.browse.sortRelevance') },
      { value: 'downloads', label: $t('modpacks.browse.sortDownloads') },
      { value: 'newest', label: $t('modpacks.browse.sortNewest') },
      { value: 'updated', label: $t('modpacks.browse.sortUpdated') },
    ]}
    sortTestid="modpack-sort-select"
    activeCount={activeCount(filterFacets)}
    expanded={drawerOpen}
    onSearchInput={(v) => (query = v)}
    onSortChange={(v) => (sortChoice = v as ModpackSort)}
    onOpenDrawer={() => (drawerOpen = true)}
  />
  <BrowseFilterChips
    chips={activeChips(filterFacets)}
    onClear={clearChip}
    onClearAll={clearAllFilters}
    clearAllTestid="modpack-clear-filters"
  />
</div>

<BrowseFilterDrawer
  bind:open={drawerOpen}
  bind:loader={loaderFilter}
  bind:mc={mcFilter}
  bind:source
  mcTestid="modpack-mc-input"
/>

<div class="px-4 pb-4">
  {#if source === 'curseforge' && needsCfKey}
    <CurseForgeKeyBanner onOpenSettings={() => (settingsOpen.value = { tab: 'curseforge' })} />
  {:else if loading}
    <div class="flex justify-center py-8 text-secondary">
      <Spinner size="lg" label={$t('modpacks.browse.searching')} />
    </div>
  {:else if error}
    <div class="mt-4 text-sm text-danger">{error}</div>
  {:else if page && page.hits.length === 0}
    <div class="mt-8 text-sm text-placeholder text-center">{$t('modpacks.browse.noResults')}</div>
  {:else if page}
    {#if browserPrefs.layout === 'grid'}
      <div class="mt-2 grid grid-cols-1 sm:grid-cols-2 gap-2">
        {#each sortedHits as hit (hit.project_id)}
          <ModpackCard
            {hit}
            layout="grid"
            onClick={() => onPickHit(hit, mcFilter.trim() || null)}
          />
        {/each}
      </div>
    {:else}
      <div class="mt-2 flex flex-col border border-border-subtle rounded overflow-hidden">
        {#each sortedHits as hit (hit.project_id)}
          <ModpackCard
            {hit}
            layout="list"
            onClick={() => onPickHit(hit, mcFilter.trim() || null)}
          />
        {/each}
      </div>
    {/if}
    <!-- Steam-style footer: page nav centered, per-page selector on the right. -->
    <div class="mt-4 flex items-center gap-3 text-sm text-muted">
      <span class="flex-1"></span>
      <button
        type="button"
        class="btn-secondary btn-sm"
        disabled={pageNum === 0}
        onclick={() => (pageNum -= 1)}
      >
        {$t('modpacks.browse.prev')}
      </button>
      <span>
        {$t('modpacks.browse.pageOf', {
          page: pageNum + 1,
          total: Math.max(1, Math.ceil(page.total / browserPrefs.pageSize)),
        })}
      </span>
      <button
        type="button"
        class="btn-secondary btn-sm"
        disabled={(pageNum + 1) * browserPrefs.pageSize >= page.total}
        onclick={() => (pageNum += 1)}
      >
        {$t('modpacks.browse.next')}
      </button>
      <span class="flex-1 flex justify-end">
        <PageSizePicker />
      </span>
    </div>
  {/if}
</div>
