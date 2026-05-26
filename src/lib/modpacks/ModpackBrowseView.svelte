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
  import CurseForgeKeyBanner from '$lib/mods/CurseForgeKeyBanner.svelte';
  import McVersionCombobox from '$lib/mods/McVersionCombobox.svelte';
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
    runSearch();
  });

  // Reset paginator when filters (NOT pageNum itself) change — otherwise
  // a narrowed query could land the user on an empty page mid-list.
  let prevFilters = $state('');
  $effect(() => {
    const fp = `${source}|${query}|${mcFilter}|${loaderFilter}|${sortChoice}`;
    if (fp !== prevFilters) {
      prevFilters = fp;
      if (pageNum !== 0) pageNum = 0;
    }
  });
</script>

<div class="p-4 pb-2 flex flex-wrap gap-2">
  <SourcePicker bind:value={source} />
  <input
    type="search"
    bind:value={query}
    placeholder={source === 'curseforge'
      ? 'Search modpacks on CurseForge...'
      : 'Search modpacks on Modrinth...'}
    class="flex-1 min-w-[10rem] px-3 py-2 border rounded text-sm"
    data-testid="modpack-search-input"
  />
  <McVersionCombobox bind:value={mcFilter} dataTestid="modpack-mc-input" />
  <select
    bind:value={loaderFilter}
    class="px-3 py-2 border rounded text-sm"
    data-testid="modpack-loader-select"
  >
    <option value="">All loaders</option>
    <option value="fabric">Fabric</option>
    <option value="quilt">Quilt</option>
    <option value="forge">Forge</option>
    <option value="neoforge">NeoForge</option>
  </select>
  <select
    bind:value={sortChoice}
    class="px-3 py-2 border rounded text-sm"
    data-testid="modpack-sort-select"
  >
    <option value="relevance">Sort: relevance</option>
    <option value="downloads">Sort: downloads</option>
    <option value="newest">Sort: newest</option>
    <option value="updated">Sort: updated</option>
  </select>
  <button
    type="button"
    class="text-xs text-neutral-600 underline hover:text-neutral-900 disabled:opacity-40 disabled:no-underline"
    disabled={!mcFilter && !loaderFilter}
    data-testid="modpack-clear-filters"
    onclick={() => {
      mcFilter = '';
      loaderFilter = '';
    }}
  >
    Clear filters
  </button>
</div>

<div class="px-4 pb-4">
  {#if source === 'curseforge' && needsCfKey}
    <CurseForgeKeyBanner onOpenSettings={() => (settingsOpen.value = { tab: 'curseforge' })} />
  {:else if loading}
    <div class="mt-4 text-sm text-neutral-500">Searching...</div>
  {:else if error}
    <div class="mt-4 text-sm text-red-600">{error}</div>
  {:else if page && page.hits.length === 0}
    <div class="mt-8 text-sm text-neutral-400 text-center">No modpacks found.</div>
  {:else if page}
    <div class="mt-2 grid grid-cols-1 sm:grid-cols-2 gap-2">
      {#each page.hits as hit (hit.project_id)}
        <ModpackCard {hit} onClick={() => onPickHit(hit, mcFilter.trim() || null)} />
      {/each}
    </div>
    <div class="mt-4 flex justify-between text-sm">
      <button
        type="button"
        class="px-2 py-1 border rounded disabled:opacity-50"
        disabled={pageNum === 0}
        onclick={() => (pageNum -= 1)}
      >
        ← Previous
      </button>
      <span class="text-neutral-500">
        Page {pageNum + 1} of {Math.max(1, Math.ceil(page.total / 20))}
      </span>
      <button
        type="button"
        class="px-2 py-1 border rounded disabled:opacity-50"
        disabled={(pageNum + 1) * 20 >= page.total}
        onclick={() => (pageNum += 1)}
      >
        Next →
      </button>
    </div>
  {/if}
</div>
