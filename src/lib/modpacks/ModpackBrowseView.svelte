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
  import { browserPrefs, PAGE_SIZES } from '$lib/mods/browser-prefs.svelte';
  import CurseForgeKeyBanner from '$lib/mods/CurseForgeKeyBanner.svelte';
  import LayoutToggle from '$lib/mods/LayoutToggle.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import McVersionCombobox from '$lib/mods/McVersionCombobox.svelte';
  import { prioritizeByTitle } from '$lib/mods/search-rank';
  import SourcePicker from '$lib/mods/SourcePicker.svelte';
  import ModpackCard from './ModpackCard.svelte';

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
  let loaderFilter = $state<'' | 'fabric' | 'quilt' | 'forge' | 'neoforge'>('');

  let sortChoice = $state<ModpackSort>('relevance');
  let page = $state<ModpackSearchPage | null>(null);
  let pageNum = $state(0);
  let loading = $state(false);
  let error = $state<string | null>(null);
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

<div class="p-4 pb-2 flex flex-wrap gap-2 items-center" data-tour-ctx="modpacks-filters">
  <SourcePicker bind:value={source} />
  <input
    type="search"
    bind:value={query}
    placeholder={source === 'curseforge'
      ? 'Search modpacks on CurseForge...'
      : 'Search modpacks on Modrinth...'}
    class="filter-control flex-1 min-w-[10rem]"
    data-testid="modpack-search-input"
  />
  <McVersionCombobox bind:value={mcFilter} dataTestid="modpack-mc-input" />
  <select
    bind:value={loaderFilter}
    class="filter-control filter-control-select filter-select"
    class:is-empty={!loaderFilter}
    data-testid="modpack-loader-select"
  >
    <option value="">Any</option>
    <option value="fabric">Fabric</option>
    <option value="quilt">Quilt</option>
    <option value="forge">Forge</option>
    <option value="neoforge">NeoForge</option>
  </select>
  <label class="text-sm text-secondary inline-flex items-center gap-1">
    Sort:
    <select
      bind:value={sortChoice}
      class="filter-control filter-control-select"
      data-testid="modpack-sort-select"
    >
      <option value="relevance">Relevance</option>
      <option value="downloads">Downloads</option>
      <option value="newest">Newest</option>
      <option value="updated">Updated</option>
    </select>
  </label>
  <button
    type="button"
    class="btn-tertiary text-xs"
    disabled={!mcFilter && !loaderFilter}
    data-testid="modpack-clear-filters"
    onclick={() => {
      mcFilter = '';
      loaderFilter = '';
    }}
  >
    Clear filters
  </button>
  <select
    class="filter-control filter-control-select"
    bind:value={browserPrefs.pageSize}
    data-testid="modpack-page-size"
  >
    {#each PAGE_SIZES as n}
      <option value={n}>{n} / page</option>
    {/each}
  </select>
  <LayoutToggle />
</div>

<div class="px-4 pb-4">
  {#if source === 'curseforge' && needsCfKey}
    <CurseForgeKeyBanner onOpenSettings={() => (settingsOpen.value = { tab: 'curseforge' })} />
  {:else if loading}
    <div class="flex justify-center py-8 text-secondary">
      <Spinner size="lg" label="Searching…" />
    </div>
  {:else if error}
    <div class="mt-4 text-sm text-danger">{error}</div>
  {:else if page && page.hits.length === 0}
    <div class="mt-8 text-sm text-placeholder text-center">No modpacks found.</div>
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
    <div class="mt-4 flex justify-between text-sm">
      <button
        type="button"
        class="btn-secondary btn-sm"
        disabled={pageNum === 0}
        onclick={() => (pageNum -= 1)}
      >
        ← Previous
      </button>
      <span class="text-muted">
        Page {pageNum + 1} of {Math.max(1, Math.ceil(page.total / browserPrefs.pageSize))}
      </span>
      <button
        type="button"
        class="btn-secondary btn-sm"
        disabled={(pageNum + 1) * browserPrefs.pageSize >= page.total}
        onclick={() => (pageNum += 1)}
      >
        Next →
      </button>
    </div>
  {/if}
</div>
