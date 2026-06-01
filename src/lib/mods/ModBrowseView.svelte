<script lang="ts">
  import {
    commands,
    type InstalledMod,
    type LoaderKind,
    type ModSort,
    type ModSource,
    type ModSummary,
    type ModVersion,
  } from '$lib/ipc/bindings';
  import { untrack } from 'svelte';
  import { formatError } from '$lib/ipc/format-error';
  import { prioritizeByTitle } from '$lib/mods/search-rank';
  import { browserPrefs } from './browser-prefs.svelte';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { cfKeyVersion, settingsOpen } from '$lib/settings/state.svelte';
  import CurseForgeKeyBanner from './CurseForgeKeyBanner.svelte';
  import DependencyDialog from './DependencyDialog.svelte';
  import PageSizePicker from './PageSizePicker.svelte';
  import ModCard from './ModCard.svelte';
  import ModDetailModal from './ModDetailModal.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import BrowseFilterBar from '$lib/browse/BrowseFilterBar.svelte';
  import BrowseFilterChips from '$lib/browse/BrowseFilterChips.svelte';
  import BrowseFilterDrawer from '$lib/browse/BrowseFilterDrawer.svelte';
  import { activeChips, activeCount, type FilterChipKey } from '$lib/browse/filter-model';

  // The Browse pane inside ModBrowserTab. Responsibilities:
  //   - Render the shared filter toolbar (search 300ms debounced, sort)
  //     with a Filters drawer (loader, MC version, Show installed) and a
  //     removable applied-filter chip row. Loader + MC pre-fill from the
  //     active instance when one is selected.
  //   - Page through results with Prev / Next.
  //   - Detect when source = CurseForge and no API key is stored, and
  //     swap the whole search UI for CurseForgeKeyBanner — both at
  //     mount (via mods_get_curseforge_key_status) and on demand if a
  //     search returns ModsPlatformAuth.
  //   - Drive the install flow: fetch versions → resolve deps. If no
  //     deps surface, install directly. Otherwise open the dependency
  //     dialog (which lands in Task 16; for now we just hold the
  //     prompt state).
  //
  // ModDetailDrawer (Task 15) and DependencyDialog (Task 16) will read
  // `drawerProject` and `depPrompt` respectively. Their state lives
  // here so this file is the single source of truth for those flows
  // once those tasks land.

  let {
    source,
    instanceId,
    mcVersion,
    loader,
  }: {
    source: ModSource;
    instanceId: string | null;
    mcVersion: string | null;
    loader: LoaderKind | null;
  } = $props();

  let query = $state('');
  // Filters mirror the active instance's MC + loader. They re-sync
  // whenever the user switches to a different instance — keeping a
  // stale filter from a previous instance would surface mods that
  // can't run on the now-active loader. User edits stick within the
  // same instance session; switching instances always resets.
  let mcFilter = $state('');
  let loaderFilter = $state<LoaderKind | ''>('');

  $effect(() => {
    mcFilter = mcVersion ?? '';
    // A vanilla instance has no loader, so collapsing it to '' (the
    // "—" entry in the dropdown) means the search drops the loader
    // facet entirely. Modrinth has no "minecraft" mod category, so
    // sending loader='vanilla' would yield ~0 results — a confusing
    // empty Mod browser on vanilla instances. The Install path still
    // shows a loader-incompatibility warning, so we don't silently
    // mislead users.
    loaderFilter = loader && loader !== 'vanilla' ? loader : '';
  });
  let sort = $state<ModSort>('downloads');
  // Default: include installed mods in the result list (current
  // behaviour). Toggle off → filter them out client-side for a
  // cleaner "discovery" view. The pagination counter still reflects
  // the platform's total, so an early page may render fewer cards
  // when many of its hits are already installed.
  let showInstalled = $state(true);
  const pageSize = $derived(browserPrefs.pageSize);
  // Reset to page 1 when the user changes the page size so we never
  // land on an out-of-range page. The existing buffer is kept — future
  // fetches will use the new chunk size automatically.
  let prevPageSize = $state(browserPrefs.pageSize);
  $effect(() => {
    if (browserPrefs.pageSize !== prevPageSize) {
      prevPageSize = browserPrefs.pageSize;
      displayPage = 1;
    }
  });
  // A single fill (fresh search or Next) fetches at most this many
  // platform pages before yielding — bounds the request burst on an
  // instance where most search hits are already installed.
  const MAX_FETCHES_PER_FILL = 8;

  // Accumulating buffer of every hit fetched for the current search, in
  // platform order. "Show installed" filters this buffer at render time
  // and pagination runs over the filtered view — so pages stay uniform
  // and unchecking the filter never dead-ends on an empty page.
  let buffer = $state<ModSummary[]>([]);
  let total = $state(0);
  let exhausted = $state(false);
  let displayPage = $state(1);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let drawerProject = $state<string | null>(null);
  let drawerOpen = $state(false);
  // Dependencies promoted to the dialog carry the project's display name
  // alongside the version (the version's own `name` field is the release
  // title, not the mod name — distinct on Modrinth and confusing in the
  // dialog).
  type DepItem = { version: ModVersion; projectName: string; projectSource: ModSource };
  // OptionalItem extends DepItem with the optional's own transitive required
  // sub-deps (enriched to DepItem). The dialog renders these indented under
  // the checkbox so the user can see what gets pulled in when they opt-in.
  type OptionalItem = DepItem & { requires: DepItem[] };
  let depPrompt = $state<{
    primary: ModVersion;
    primaryProjectName: string;
    required: DepItem[];
    optional: OptionalItem[];
    incompatible: string[];
    unresolvable: string[];
    // Loader projects (NeoForge, Fabric, etc.) that the mod declared as
    // dependencies. We don't install them — loaders are managed at the
    // instance level — but we still show them so the user knows "this
    // mod targets NeoForge".
    loaderRequirements: string[];
    // Set when the picked version's loaders don't include the active
    // instance's loader (or the instance is vanilla). Installing
    // anyway will leave a jar in the mods folder that Minecraft can't
    // load. Surfaced as a red warning row in the dialog.
    loaderMismatch: { instanceLoader: string; modLoaders: LoaderKind[] } | null;
  } | null>(null);

  let needsCfKey = $state(false);
  // Track which Modrinth / CurseForge projects are already installed
  // in the active instance. Each entry is paired with the project's
  // display name (looked up via mods_project) because installed-mods.json
  // stores the version's release title in `name` (e.g.
  // "v13.0.121 for Forge 1.20.4"), not the project name (e.g.
  // "Cloth Config API"). Cross-platform matching needs the latter.
  type InstalledRow = { installed: InstalledMod; projectName: string | null };
  let installedMods = $state<InstalledRow[]>([]);

  async function refreshInstalled() {
    if (!instanceId) {
      installedMods = [];
      return;
    }
    const r = await commands.modsListInstalled(instanceId);
    if (r.status !== 'ok') return;
    // Look up project names in parallel. Manual mods (source: null)
    // skip the call and keep projectName: null — they only ever match
    // via the exact-id path anyway.
    installedMods = await Promise.all(
      r.data.map(async (m): Promise<InstalledRow> => {
        if (m.source === null || m.project_id === null) {
          return { installed: m, projectName: null };
        }
        const p = await commands.modsProject(m.source as ModSource, m.project_id);
        return {
          installed: m,
          projectName: p.status === 'ok' ? p.data.summary.name : null,
        };
      }),
    );
  }

  $effect(() => {
    // biome-ignore lint/correctness/noUnusedVariables: reactive read
    const _id = instanceId;
    void refreshInstalled();
  });

  // Reduce a mod's display name to a comparison key. Platforms often
  // append loader/edition suffixes ("Cloth Config API (Forge)",
  // "Sodium Fabric Mod", …) inconsistently, so we strip those words
  // before comparing. Falls through to a lowercased alphanumeric-only
  // string. Returns '' for names that collapse to nothing (extremely
  // rare and harmless — they just won't cross-platform-match).
  const NAME_NOISE = /\b(api|fabric|forge|neoforge|quilt|mod|edition|for)\b/g;
  function nameKey(s: string): string {
    return s
      .toLowerCase()
      .replace(NAME_NOISE, '')
      .replace(/[^a-z0-9]+/g, '');
  }

  function installedFor(card: ModSummary): InstalledMod | null {
    // Exact platform-and-id match first.
    const exact = installedMods.find(
      (r) => r.installed.source === card.source && r.installed.project_id === card.project_id,
    );
    if (exact) return exact.installed;
    // Cross-platform fallback against the fetched project name. We
    // normalize both sides (strip platform suffixes, lowercase,
    // alphanumeric-only) so "Cloth Config API" matches "Cloth Config
    // API (Forge)" matches "Cloth Config Fabric Edition" matches just
    // "Cloth Config". False positives are bounded — "Sodium" vs
    // "Sodium Extra" still differ.
    const cardKey = nameKey(card.name);
    if (cardKey === '') return null;
    return (
      installedMods.find((r) => r.projectName !== null && nameKey(r.projectName) === cardKey)
        ?.installed ?? null
    );
  }

  // Apply the "Show installed" filter to a hit list. Single source of
  // the filter rule — used by the `filteredHits` derived and by the
  // fill loop (a plain function so the loop never depends on a derived
  // re-evaluating mid-iteration).
  function applyInstalledFilter(hits: ModSummary[]): ModSummary[] {
    return showInstalled ? hits : hits.filter((h) => installedFor(h) === null);
  }

  // The buffer narrowed by "Show installed", re-ranked so name-matches
  // come first (see prioritizeByTitle for the rationale), then sliced
  // for the current page. `hasNext` is true when there is another page
  // to show, or more platform results might still be fetched.
  const filteredHits = $derived(
    prioritizeByTitle(applyInstalledFilter(buffer), query, (h) => h.name),
  );
  const pageHits = $derived(
    filteredHits.slice((displayPage - 1) * pageSize, displayPage * pageSize),
  );
  const hasNext = $derived(filteredHits.length > displayPage * pageSize || !exhausted);

  async function uninstallCard(card: ModSummary) {
    if (!instanceId) return;
    const inst = installedFor(card);
    if (!inst) return;
    const r = await commands.modsUninstall(instanceId, inst.sha1);
    if (r.status === 'error') {
      error = formatError(r.error);
      return;
    }
    await refreshInstalled();
  }

  async function toggleCard(card: ModSummary) {
    if (!instanceId) return;
    const inst = installedFor(card);
    if (!inst) return;
    const r = inst.enabled
      ? await commands.modsDisable(instanceId, inst.sha1)
      : await commands.modsEnable(instanceId, inst.sha1);
    if (r.status === 'error') {
      error = formatError(r.error);
      return;
    }
    await refreshInstalled();
  }

  async function refreshCfKey() {
    if (source !== 'curseforge') {
      needsCfKey = false;
      return;
    }
    const wasGated = needsCfKey;
    const s = await commands.modsGetCurseforgeKeyStatus();
    needsCfKey = s.status === 'ok' ? s.data === 'missing' : true;
    // Banner just lifted (e.g. the user saved a key in Settings). The
    // search-trigger $effect won't re-run on its own because none of
    // its watched filters changed, so kick off a search manually.
    if (wasGated && !needsCfKey) {
      void resetSearch();
    }
  }

  // Re-poll the CF key status whenever the user flips the platform
  // dropdown OR whenever the Settings form bumps cfKeyVersion (save /
  // clear). The latter is what makes the banner disappear the moment
  // the user adds a key, without having to manually re-mount the view.
  // Both inputs share one effect so refreshCfKey only fires once per
  // change instead of twice.
  $effect(() => {
    // biome-ignore lint/correctness/noUnusedVariables: reactive reads
    const _s = source;
    // biome-ignore lint/correctness/noUnusedVariables: reactive reads
    const _v = cfKeyVersion.value;
    void refreshCfKey();
  });

  $effect(() => {
    // Touch the filters so the effect re-runs when they change.
    // biome-ignore lint/correctness/noUnusedVariables: reactive read
    const _src = source;
    // biome-ignore lint/correctness/noUnusedVariables: reactive read
    const _sort = sort;
    // biome-ignore lint/correctness/noUnusedVariables: reactive read
    const _mc = mcFilter;
    // biome-ignore lint/correctness/noUnusedVariables: reactive read
    const _ld = loaderFilter;
    // `untrack` prevents the async work in `resetSearch` / `fill` from
    // registering `buffer`, `exhausted`, etc. as dependencies of this
    // effect, which would create an update cycle (write → re-run → write).
    untrack(() => void resetSearch());
  });

  // Fetch the next contiguous platform page into `buffer`. Returns
  // 'ok' | 'auth' | 'error'; updates `total` and `exhausted`.
  async function fetchNextPlatformPage(): Promise<'ok' | 'auth' | 'error'> {
    const result = await commands.modsSearch({
      source,
      query,
      // Empty input strings collapse to `null` — the search backend
      // treats null as "no MC / no loader facet", same as the old
      // "Show all" checkbox did when checked. Clearing both fields
      // (or hitting Clear filters) is the explicit way to widen.
      mc_version: mcFilter || null,
      loader: (loaderFilter || null) as LoaderKind | null,
      sort,
      page_size: pageSize,
      offset: buffer.length,
    });
    if (result.status === 'ok') {
      buffer = [...buffer, ...result.data.hits];
      total = result.data.total;
      exhausted = buffer.length >= total;
      return 'ok';
    }
    if (result.error.kind === 'mods_platform_auth') {
      return 'auth';
    }
    error = formatError(result.error);
    return 'error';
  }

  // Fetch platform pages until `filteredHits` covers `targetPage`
  // display pages, the platform is exhausted, or the per-fill cap is
  // hit. Drives `loading`.
  async function fill(targetPage: number) {
    loading = true;
    error = null;
    let fetches = 0;
    while (
      applyInstalledFilter(buffer).length < targetPage * pageSize &&
      !exhausted &&
      fetches < MAX_FETCHES_PER_FILL
    ) {
      const r = await fetchNextPlatformPage();
      fetches += 1;
      if (r === 'auth') {
        needsCfKey = true;
        buffer = [];
        loading = false;
        return;
      }
      if (r === 'error') {
        loading = false;
        return;
      }
    }
    loading = false;
  }

  // Start a fresh search: drop the buffer and fetch from offset 0.
  async function resetSearch() {
    buffer = [];
    total = 0;
    exhausted = false;
    displayPage = 1;
    if (needsCfKey) return;
    await fill(1);
  }

  async function next() {
    const target = displayPage + 1;
    await fill(target);
    // Advance only if the target page actually has a card — a
    // cap-limited or exhausted fill may not have reached it.
    if (applyInstalledFilter(buffer).length > (target - 1) * pageSize) {
      displayPage = target;
    }
  }

  function prev() {
    if (displayPage > 1) displayPage -= 1;
  }

  // Single entry point for flipping "Show installed": the drawer toggle
  // and the chip's × both call this so the re-paging stays in one place.
  async function setShowInstalled(value: boolean) {
    showInstalled = value;
    displayPage = 1;
    await fill(1);
  }

  const filterFacets = $derived({ loader: loaderFilter, mc: mcFilter, showInstalled });

  function clearChip(key: FilterChipKey) {
    if (key === 'loader') loaderFilter = '';
    else if (key === 'mc') mcFilter = '';
    else if (key === 'showInstalled') void setShowInstalled(true);
  }

  function clearAllFilters() {
    loaderFilter = '';
    mcFilter = '';
    void setShowInstalled(true);
  }

  let debounceTimer: number | undefined;
  // Adapter so the debounced search keeps working with the bar's string
  // callback (the old handler read e.target.value off the event).
  function onSearchInput(value: string) {
    query = value;
    if (debounceTimer) window.clearTimeout(debounceTimer);
    debounceTimer = window.setTimeout(() => {
      void resetSearch();
    }, 300);
  }

  async function startInstall(card: ModSummary, pinnedVersion?: ModVersion) {
    if (!instanceId || !mcVersion || !loader) {
      error = 'No active instance';
      return;
    }
    if (loader === 'vanilla') {
      error =
        'This instance is vanilla Minecraft (no mod loader). Switch to a Fabric / Quilt / Forge / NeoForge instance to install mods.';
      return;
    }
    let primary: ModVersion;
    if (pinnedVersion) {
      // Drawer passes the user's explicit choice. Skip the lookup.
      primary = pinnedVersion;
    } else {
      const versions = await commands.modsVersions(card.source, card.project_id, mcVersion, loader);
      if (versions.status === 'error') {
        error = formatError(versions.error);
        return;
      }
      if (versions.data.length === 0) {
        error = 'No compatible version found';
        return;
      }
      primary = versions.data[0]!;
    }

    // If a different version of the same project is already installed,
    // remove it first so the new version replaces it (version switch).
    const existing = installedFor(card);
    if (existing && existing.version_id !== primary.version_id) {
      const removed = await commands.modsUninstall(instanceId, existing.sha1);
      if (removed.status === 'error') {
        error = formatError(removed.error);
        return;
      }
    }
    const plan = await commands.modsResolveInstallPlan(instanceId, primary, mcVersion, loader);
    if (plan.status === 'error') {
      error = formatError(plan.error);
      return;
    }
    const p = plan.data;

    // Enrich a ModVersion to a DepItem: look up the project's display name
    // via modsProject, falling back to the version's own `name` field when
    // the platform lookup fails (network, deleted project, etc.).
    const enrichDep = async (v: ModVersion): Promise<DepItem> => {
      const proj = await commands.modsProject(v.source, v.project_id);
      return {
        version: v,
        projectName: proj.status === 'ok' ? proj.data.summary.name : v.name,
        projectSource: v.source,
      };
    };

    // Look up a human-readable name for a DepProjectRef. Falls back to the
    // raw project_id / mod_id string when the platform lookup fails.
    type DepRef = (typeof p.incompatible)[number];
    const enrichRefName = async (r: DepRef): Promise<string> => {
      const refSource: ModSource = 'project_id' in r ? 'modrinth' : 'curseforge';
      const id = 'project_id' in r ? r.project_id : String(r.mod_id);
      const proj = await commands.modsProject(refSource, id);
      return proj.status === 'ok' ? proj.data.summary.name : id;
    };

    // Enrich required deps (plain DepItem list — backend already pruned loaders
    // and already-installed entries).
    const requiredEnriched = await Promise.all(p.required.map(enrichDep));

    // Enrich optional deps: each OptionalDep carries a `requires` sub-list
    // (the optional's own transitive requireds). Enrich both the top-level
    // version and its requires list so the dialog can reveal sub-deps live.
    const optionalEnriched: OptionalItem[] = await Promise.all(
      p.optional.map(async (o): Promise<OptionalItem> => {
        const top = await enrichDep(o.version);
        const subReqs = await Promise.all(o.requires.map(enrichDep));
        return { ...top, requires: subReqs };
      }),
    );

    // Enrich incompatible / unresolvable DepProjectRefs to display names.
    const incompatibleNames = await Promise.all(p.incompatible.map(enrichRefName));
    const unresolvableNames = await Promise.all(p.unresolvable.map(enrichRefName));

    // Enrich loader_requirements refs to display names (informational).
    const loaderRequirements = await Promise.all(p.loader_requirements.map(enrichRefName));

    const primaryProject = await commands.modsProject(primary.source, primary.project_id);
    const primaryProjectName =
      primaryProject.status === 'ok' ? primaryProject.data.summary.name : primary.name;

    // Loader mismatch detection: if the version reports loaders and the
    // active instance's loader isn't one of them (or the instance is
    // vanilla, which has no loader at all), the jar won't load at
    // runtime. We don't block — the user might be testing — but we
    // force the dialog open with a red warning.
    const loaderMismatch =
      primary.loaders.length > 0 && !primary.loaders.includes(loader)
        ? { instanceLoader: loader, modLoaders: primary.loaders }
        : null;

    // Fast path: nothing to show → install directly.
    if (
      requiredEnriched.length === 0 &&
      optionalEnriched.length === 0 &&
      p.incompatible.length === 0 &&
      p.unresolvable.length === 0 &&
      loaderRequirements.length === 0 &&
      loaderMismatch === null
    ) {
      const installed = await commands.modsInstallWithDeps(
        instanceId,
        { source: primary.source, project_id: primary.project_id, version_id: primary.version_id },
        [],
      );
      if (installed.status === 'error') {
        pushWarning('Mod install failed', [formatError(installed.error)]);
      } else {
        // Fast path has no dependencies; use the resolved project name (not
        // the backend's release-title `primary_name`) for the toast title.
        pushSuccess(`Installed ${primaryProjectName}`, []);
        await refreshInstalled();
      }
    } else {
      depPrompt = {
        primary,
        primaryProjectName,
        required: requiredEnriched,
        optional: optionalEnriched,
        incompatible: incompatibleNames,
        unresolvable: unresolvableNames,
        loaderRequirements,
        loaderMismatch,
      };
    }
  }
</script>

{#if loader === 'vanilla'}
  <div
    class="p-6 bg-warning-bg border border-warning-text/30 rounded mx-3 my-4 text-sm text-warning-text"
  >
    <div class="font-medium mb-1">This instance is vanilla Minecraft</div>
    <p class="text-warning-text">
      Vanilla Minecraft has no mod loader, so mod jars from Modrinth / CurseForge cannot be loaded
      at runtime. To install mods, create or switch to a Fabric, Quilt, Forge, or NeoForge instance
      via the Manage panel in the sidebar.
    </p>
  </div>
{:else if needsCfKey}
  <CurseForgeKeyBanner onOpenSettings={() => (settingsOpen.value = { tab: 'curseforge' })} />
{:else}
  <div class="sticky top-0 z-10 bg-base border-b border-border-subtle">
    <BrowseFilterBar
      searchAriaLabel="Search mods"
      searchPlaceholder="Search mods..."
      {sort}
      sortOptions={[
        { value: 'downloads', label: 'Downloads' },
        { value: 'relevance', label: 'Relevance' },
        { value: 'updated', label: 'Updated' },
      ]}
      activeCount={activeCount(filterFacets)}
      expanded={drawerOpen}
      {onSearchInput}
      onSortChange={(v) => (sort = v as ModSort)}
      onOpenDrawer={() => (drawerOpen = true)}
    />
    <BrowseFilterChips
      chips={activeChips(filterFacets)}
      onClear={clearChip}
      onClearAll={clearAllFilters}
      clearAllTestid="mod-clear-filters"
    />
  </div>

  <BrowseFilterDrawer
    bind:open={drawerOpen}
    bind:loader={loaderFilter}
    bind:mc={mcFilter}
    {showInstalled}
    onShowInstalledChange={(v) => void setShowInstalled(v)}
  />

  <div class="p-3 space-y-2">
    {#if error}
      <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2">{error}</div>
    {/if}
    {#if loading}
      <div class="flex justify-center py-8 text-secondary">
        <Spinner size="lg" label="Searching…" />
      </div>
    {:else if pageHits.length > 0}
      {#if browserPrefs.layout === 'grid'}
        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-2 items-stretch">
          {#each pageHits as hit (`${hit.source}:${hit.project_id}`)}
            <ModCard
              summary={hit}
              installed={installedFor(hit)}
              onInstall={() => startInstall(hit)}
              onOpenDetail={() => (drawerProject = hit.project_id)}
              onToggle={() => toggleCard(hit)}
              onUninstall={() => uninstallCard(hit)}
              layout="grid"
            />
          {/each}
        </div>
      {:else}
        <div class="mt-1 flex flex-col border border-border-subtle rounded overflow-hidden">
          {#each pageHits as hit (`${hit.source}:${hit.project_id}`)}
            <ModCard
              summary={hit}
              installed={installedFor(hit)}
              onInstall={() => startInstall(hit)}
              onOpenDetail={() => (drawerProject = hit.project_id)}
              onToggle={() => toggleCard(hit)}
              onUninstall={() => uninstallCard(hit)}
              layout="list"
            />
          {/each}
        </div>
      {/if}
      <!-- Steam-style footer: page nav centered, per-page selector on the right. -->
      <div class="flex items-center gap-3 text-sm text-secondary pt-2">
        <span class="flex-1"></span>
        <button
          type="button"
          class="btn-secondary btn-sm"
          disabled={displayPage <= 1}
          onclick={prev}
        >
          ‹ Prev
        </button>
        <span>
          Page {displayPage}{showInstalled ? ` of ${Math.max(1, Math.ceil(total / pageSize))}` : ''}
        </span>
        <button type="button" class="btn-secondary btn-sm" disabled={!hasNext} onclick={next}>
          Next ›
        </button>
        <span class="flex-1 flex justify-end">
          <PageSizePicker />
        </span>
      </div>
    {:else}
      <div class="text-placeholder text-sm py-8 text-center">No results.</div>
    {/if}
  </div>

  {#if drawerProject}
    <ModDetailModal
      {source}
      projectId={drawerProject}
      {mcVersion}
      {loader}
      installedVersionId={installedMods.find(
        (r) => r.installed.source === source && r.installed.project_id === drawerProject,
      )?.installed.version_id ?? null}
      onClose={() => (drawerProject = null)}
      onInstall={(v) => {
        // Drawer passes the explicit version the user picked. We
        // re-use startInstall — it now accepts a pinnedVersion arg so
        // we skip the latest-version lookup and install exactly what
        // the user clicked. If a different version is already
        // installed for this project, startInstall handles the swap.
        drawerProject = null;
        void startInstall(
          {
            source: v.source,
            project_id: v.project_id,
            slug: null,
            name: v.name,
            summary: '',
            icon_url: null,
            downloads: null,
            author: '',
            updated_at: null,
          },
          v,
        );
      }}
    />
  {/if}
  {#if depPrompt}
    <DependencyDialog
      primary={depPrompt.primary}
      primaryProjectName={depPrompt.primaryProjectName}
      required={depPrompt.required}
      optional={depPrompt.optional}
      incompatible={depPrompt.incompatible}
      unresolvable={depPrompt.unresolvable}
      loaderRequirements={depPrompt.loaderRequirements}
      onCancel={() => (depPrompt = null)}
      onConfirm={async (chosenOptional) => {
        const prompt = depPrompt;
        if (!prompt || !instanceId) {
          depPrompt = null;
          return;
        }
        depPrompt = null;
        const installed = await commands.modsInstallWithDeps(
          instanceId,
          {
            source: prompt.primary.source,
            project_id: prompt.primary.project_id,
            version_id: prompt.primary.version_id,
          },
          chosenOptional.map((v) => ({
            source: v.source,
            project_id: v.project_id,
            version_id: v.version_id,
          })),
        );
        if (installed.status === 'error') {
          pushWarning('Mod install failed', [formatError(installed.error)]);
        } else {
          // Build the per-mod toast from the dialog's already-resolved project
          // names (the backend's InstallSummary carries release titles, not mod
          // names). Lines = every newly-installed dependency: the primary's
          // requireds + each chosen optional and its transitive requireds,
          // deduped by project. Matches exactly what the dialog showed.
          const seen = new Set<string>();
          const depLines: string[] = [];
          const pushDep = (name: string, source: string, projectId: string) => {
            const key = `${source}:${projectId}`;
            if (!seen.has(key)) {
              seen.add(key);
              depLines.push(name);
            }
          };
          for (const d of prompt.required) {
            pushDep(d.projectName, d.version.source, d.version.project_id);
          }
          for (const v of chosenOptional) {
            const o = prompt.optional.find(
              (x) =>
                x.version.source === v.source &&
                x.version.project_id === v.project_id &&
                x.version.version_id === v.version_id,
            );
            if (!o) continue;
            pushDep(o.projectName, o.version.source, o.version.project_id);
            for (const r of o.requires) {
              pushDep(r.projectName, r.version.source, r.version.project_id);
            }
          }
          pushSuccess(`Installed ${prompt.primaryProjectName}`, depLines);
          await refreshInstalled();
        }
      }}
    />
  {/if}
{/if}
