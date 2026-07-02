<script lang="ts">
  import { onDestroy } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { commands } from '$lib/ipc/bindings';
  import type {
    ModpackHit,
    ModpackSearchPage,
    ModpackSort,
    ModSource,
    SourceCaps,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { cfKeyVersion, settingsOpen } from '$lib/settings/state.svelte';
  import { browserPrefs } from '$lib/mods/browser-prefs.svelte';
  import { modpackBrowseState } from './browse-state.svelte';
  import CurseForgeKeyBanner from '$lib/mods/CurseForgeKeyBanner.svelte';
  import PageSizePicker from '$lib/mods/PageSizePicker.svelte';
  import Pagination from '$lib/ui/Pagination.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import { prioritizeByTitle } from '$lib/mods/search-rank';
  import BrowseFilterBar from '$lib/browse/BrowseFilterBar.svelte';
  import { activeCount } from '$lib/browse/filter-model';
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
  let {
    onPickHit,
    onQuickInstall,
    installingIds,
  }: {
    onPickHit: (hit: ModpackHit, mc: string | null) => void;
    // Optional one-click install of a pack's latest version (the parent
    // ModpacksTab supplies it). Omitted in tests/standalone use → no button.
    onQuickInstall?: (hit: ModpackHit, mc: string | null) => void;
    installingIds?: SvelteSet<string>;
  } = $props();

  // query resets to '' on each open (intentional — search box starts blank).
  let query = $state('');

  // Per-source capability flags — fetched on every source change so the UI
  // adjusts without any hardcoded `if source === 'x'` checks.
  //
  // To prevent a stale-race while the async IPC call resolves (e.g. the CF-key
  // banner incorrectly flashing during a fast FTB→CurseForge switch), caps are
  // set SYNCHRONOUSLY to known values before the await. The async fetch is the
  // authority and corrects any drift if the Rust side ever changes.
  const SOURCE_CAPS: Record<ModSource, SourceCaps> = {
    modrinth: { needs_api_key: false, supports_server_filter: true, can_export: true },
    curseforge: { needs_api_key: true, supports_server_filter: true, can_export: true },
    // Mirrors FtbModpackSource::caps() in source/ftb.rs exactly.
    ftb: { needs_api_key: false, supports_server_filter: false, can_export: false },
    // Mirrors AtlauncherModpackSource::caps() in source/atlauncher.rs exactly.
    atlauncher: { needs_api_key: false, supports_server_filter: false, can_export: false },
  };
  let caps = $state<SourceCaps>({
    needs_api_key: false,
    supports_server_filter: true,
    can_export: true,
  });
  $effect(() => {
    const source = modpackBrowseState.source;
    // Synchronous pre-set eliminates the stale-window.
    caps = SOURCE_CAPS[source];
    void (async () => {
      const r = await commands.modpackSourceCaps(source);
      if (r.status === 'ok') caps = r.data;
    })();
  });

  // CurseForge needs an API key; Modrinth is anonymous. When the user
  // picks CurseForge with no key stored, the whole search UI is
  // replaced by the key banner — same pattern as the sub-3 mod browser.
  let needsCfKey = $state(false);

  async function refreshCfKey() {
    if (!caps.needs_api_key) {
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
    void modpackBrowseState.source;
    void cfKeyVersion.value;
    void refreshCfKey();
  });

  let page = $state<ModpackSearchPage | null>(null);
  let pageNum = $state(0);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let debounce: ReturnType<typeof setTimeout> | null = null;
  // Monotonic request id so an out-of-order modpack_search response can't clobber
  // a newer page. A fast Next→Next (or edits landing back-to-back) can resolve
  // B-then-A; without this guard A would overwrite B. Mirrors ModBrowseView.
  let reqSeq = 0;

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
      if (caps.needs_api_key && needsCfKey) {
        page = null;
        return;
      }
      const seq = ++reqSeq;
      loading = true;
      error = null;
      try {
        const mc = modpackBrowseState.mcFilter.trim() || null;
        const loaderArg = modpackBrowseState.loaderFilter ? modpackBrowseState.loaderFilter : null;
        const result = await commands.modpackSearch(
          modpackBrowseState.source,
          query,
          pageNum,
          mc,
          loaderArg,
          modpackBrowseState.sortChoice,
          browserPrefs.pageSize,
        );
        // A newer search superseded this one while it awaited — drop the stale
        // result (and leave `loading` for the in-flight request to clear).
        if (seq !== reqSeq) return;
        if (result.status === 'ok') {
          page = result.data;
        } else if (result.error.kind === 'mods_platform_auth') {
          needsCfKey = true;
          page = null;
        } else {
          error = formatError(result.error);
        }
      } catch (e) {
        if (seq !== reqSeq) return;
        error = String(e);
      } finally {
        if (seq === reqSeq) loading = false;
      }
    }, 300);
  }

  // Clear a pending debounce so a search can't fire (and touch reactive state)
  // after the component has unmounted.
  onDestroy(() => {
    if (debounce) clearTimeout(debounce);
    debounce = null;
  });

  // Modpack browser has no "show installed" facet, so it never enters
  // the chip model. source is a context switch, not a chip (see
  // filter-model). Only loader + mc surface as chips here.
  const filterFacets = $derived({
    loader: modpackBrowseState.loaderFilter,
    mc: modpackBrowseState.mcFilter,
  });

  function clearAllFilters() {
    modpackBrowseState.loaderFilter = '';
    modpackBrowseState.mcFilter = '';
  }

  // Re-run search on any reactive input change.
  $effect(() => {
    void modpackBrowseState.source;
    void query;
    void modpackBrowseState.mcFilter;
    void modpackBrowseState.loaderFilter;
    void modpackBrowseState.sortChoice;
    void pageNum;
    void browserPrefs.pageSize;
    runSearch();
  });

  // Reset paginator when filters (NOT pageNum itself) change — otherwise
  // a narrowed query could land the user on an empty page mid-list.
  let prevFilters = $state('');
  $effect(() => {
    const fp = `${modpackBrowseState.source}|${query}|${modpackBrowseState.mcFilter}|${modpackBrowseState.loaderFilter}|${modpackBrowseState.sortChoice}|${browserPrefs.pageSize}`;
    if (fp !== prevFilters) {
      prevFilters = fp;
      if (pageNum !== 0) pageNum = 0;
    }
  });
</script>

<div data-tour-ctx="modpacks-filters" class="pt-2">
  <BrowseFilterBar
    searchAriaLabel={$t('modpacks.browse.searchAriaLabel')}
    searchPlaceholder={modpackBrowseState.source === 'curseforge'
      ? $t('modpacks.browse.searchPlaceholderCurseForge')
      : modpackBrowseState.source === 'ftb'
        ? $t('modpacks.browse.searchPlaceholderFtb')
        : modpackBrowseState.source === 'atlauncher'
          ? $t('modpacks.browse.searchPlaceholderAtlauncher')
          : $t('modpacks.browse.searchPlaceholderModrinth')}
    searchTestid="modpack-search-input"
    sort={modpackBrowseState.sortChoice}
    sortOptions={[
      { value: 'relevance', label: $t('modpacks.browse.sortRelevance') },
      { value: 'downloads', label: $t('modpacks.browse.sortDownloads') },
      { value: 'newest', label: $t('modpacks.browse.sortNewest') },
      { value: 'updated', label: $t('modpacks.browse.sortUpdated') },
    ]}
    sortTestid="modpack-sort-select"
    showLoader={true}
    bind:loader={modpackBrowseState.loaderFilter}
    bind:mc={modpackBrowseState.mcFilter}
    mcTestid="modpack-mc-input"
    serverFilters={caps.supports_server_filter}
    serverFilterNote={$t('modpacks.browse.ftbClientFilterNote')}
    activeCount={activeCount(filterFacets)}
    onClearAll={clearAllFilters}
    onSearchInput={(v) => (query = v)}
    onSortChange={(v) => (modpackBrowseState.sortChoice = v as ModpackSort)}
  />
</div>

<div class="px-4 pb-4">
  {#if caps.needs_api_key && needsCfKey}
    <CurseForgeKeyBanner onOpenSettings={() => (settingsOpen.value = { tab: 'integrations' })} />
  {:else if loading}
    <LoadingPanel label={$t('modpacks.browse.searching')} delayMs={0} />
  {:else if error}
    <div class="mt-4 text-sm text-danger flex items-center justify-between gap-3">
      <span>{error}</span>
      <button type="button" class="btn-secondary btn-sm shrink-0" onclick={() => runSearch()}>
        {$t('modpacks.browse.errorRetry')}
      </button>
    </div>
  {:else if page && page.hits.length === 0}
    <div class="mt-8 text-sm text-placeholder text-center">{$t('modpacks.browse.noResults')}</div>
  {:else if page}
    {#if browserPrefs.layout === 'grid'}
      <div class="mt-2 grid grid-cols-1 sm:grid-cols-2 gap-2">
        {#each sortedHits as hit (hit.project_id)}
          <ModpackCard
            {hit}
            layout="grid"
            onClick={() => onPickHit(hit, modpackBrowseState.mcFilter.trim() || null)}
            onQuickInstall={onQuickInstall
              ? () => onQuickInstall(hit, modpackBrowseState.mcFilter.trim() || null)
              : null}
            installing={installingIds?.has(hit.project_id) ?? false}
          />
        {/each}
      </div>
    {:else}
      <div class="mt-2 flex flex-col border border-border-subtle rounded overflow-hidden">
        {#each sortedHits as hit (hit.project_id)}
          <ModpackCard
            {hit}
            layout="list"
            onClick={() => onPickHit(hit, modpackBrowseState.mcFilter.trim() || null)}
            onQuickInstall={onQuickInstall
              ? () => onQuickInstall(hit, modpackBrowseState.mcFilter.trim() || null)
              : null}
            installing={installingIds?.has(hit.project_id) ?? false}
          />
        {/each}
      </div>
    {/if}
    <!-- Steam-style footer: shared pagination control, per-page selector right. -->
    <Pagination
      page={pageNum}
      pageCount={Math.max(1, Math.ceil(page.total / browserPrefs.pageSize))}
      disabled={loading}
      onPage={(n) => (pageNum = n)}
    >
      {#snippet end()}
        <PageSizePicker />
      {/snippet}
    </Pagination>
  {/if}
</div>
