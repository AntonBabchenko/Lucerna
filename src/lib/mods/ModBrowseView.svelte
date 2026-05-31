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
  import { browserPrefs, PAGE_SIZES } from './browser-prefs.svelte';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { cfKeyVersion, settingsOpen } from '$lib/settings/state.svelte';
  import CurseForgeKeyBanner from './CurseForgeKeyBanner.svelte';
  import DependencyDialog from './DependencyDialog.svelte';
  import McVersionCombobox from './McVersionCombobox.svelte';
  import ModCard from './ModCard.svelte';
  import ModDetailModal from './ModDetailModal.svelte';

  // The Browse pane inside ModBrowserTab. Responsibilities:
  //   - Render a search input (300ms debounced), sort dropdown, MC
  //     version + loader filters (pre-filled from the active instance
  //     when one is selected), and a "Show all" override.
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
  // Dependencies promoted to the dialog carry the project's display name
  // alongside the version (the version's own `name` field is the release
  // title, not the mod name — distinct on Modrinth and confusing in the
  // dialog).
  type DepItem = { version: ModVersion; projectName: string; projectSource: ModSource };
  let depPrompt = $state<{
    primary: ModVersion;
    primaryProjectName: string;
    required: DepItem[];
    optional: DepItem[];
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

  // Modrinth + CurseForge both list mod loaders themselves as searchable
  // projects (e.g. modrinth.com/mod/neoforge). Some mods declare the
  // loader as a "required dependency" in their version manifest. Installing
  // the loader as a jar into {instance}/.minecraft/mods/ is wrong — loaders
  // are managed at the instance level, not as user mods. Filter dep entries
  // whose project slug matches one of these by name.
  const LOADER_SLUGS = new Set([
    'neoforge',
    'forge',
    'fabric',
    'fabric-loader',
    'quilt',
    'quilt-loader',
    'minecraft',
  ]);

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

  // Re-page when "Show installed" is toggled: a filter change resets to
  // page 1. The buffer is kept (same search); fill(1) tops it up when
  // switching OFF leaves the filtered view shorter than one page.
  async function onShowInstalledChange(e: Event) {
    showInstalled = (e.currentTarget as HTMLInputElement).checked;
    displayPage = 1;
    await fill(1);
  }

  let debounceTimer: number | undefined;
  function onQueryInput(e: Event) {
    query = (e.target as HTMLInputElement).value;
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
    const deps = await commands.modsResolveDeps(primary, mcVersion, loader);
    if (deps.status === 'error') {
      error = formatError(deps.error);
      return;
    }
    const d = deps.data;

    // Enrich each dep with its project's display name and separate out
    // dependencies that point to a known loader project (NeoForge,
    // Fabric, etc.). Loaders are managed at the instance level —
    // installing them as mod jars would produce a broken instance — but
    // we still surface them in the dialog so the user knows the mod's
    // loader target.
    type EnrichResult =
      | { kind: 'normal'; version: ModVersion; projectName: string; projectSource: ModSource }
      | { kind: 'loader'; projectName: string };

    const enrichDep = async (v: ModVersion): Promise<EnrichResult> => {
      const p = await commands.modsProject(v.source, v.project_id);
      if (p.status === 'error') {
        return { kind: 'normal', version: v, projectName: v.name, projectSource: v.source };
      }
      const slug = p.data.summary.slug ?? '';
      if (LOADER_SLUGS.has(slug.toLowerCase())) {
        return { kind: 'loader', projectName: p.data.summary.name };
      }
      return {
        kind: 'normal',
        version: v,
        projectName: p.data.summary.name,
        projectSource: p.data.summary.source,
      };
    };

    const allRequired = await Promise.all(d.required.map((r) => enrichDep(r.version)));
    const allOptional = await Promise.all(d.optional.map((o) => enrichDep(o.version)));

    // Look up a human-readable name for a DepProjectRef. Falls back
    // to the raw project_id / mod_id when the project lookup fails
    // (network, deleted project, etc.) — so the dialog never shows a
    // bare slug like "9s6osm5g" when the API call succeeded.
    type DepRef = (typeof d.incompatible)[number];
    const enrichRefName = async (r: DepRef): Promise<string> => {
      const source: ModSource = 'project_id' in r ? 'modrinth' : 'curseforge';
      const id = 'project_id' in r ? r.project_id : String(r.mod_id);
      const p = await commands.modsProject(source, id);
      return p.status === 'ok' ? p.data.summary.name : id;
    };
    const incompatibleNames = await Promise.all(d.incompatible.map(enrichRefName));
    const unresolvableNames = await Promise.all(d.unresolvable.map(enrichRefName));
    const requiredEnriched = allRequired.filter(
      (x): x is Extract<EnrichResult, { kind: 'normal' }> => x.kind === 'normal',
    );
    const optionalEnriched = allOptional.filter(
      (x): x is Extract<EnrichResult, { kind: 'normal' }> => x.kind === 'normal',
    );
    const loaderRequirements = Array.from(
      new Set(
        [...allRequired, ...allOptional]
          .filter((x): x is Extract<EnrichResult, { kind: 'loader' }> => x.kind === 'loader')
          .map((x) => x.projectName),
      ),
    );

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

    // Open the dialog whenever we have anything to show — including a
    // pure "this mod targets NeoForge" loader-only requirement or a
    // loader mismatch, both of which are informational but worth
    // surfacing.
    if (
      requiredEnriched.length === 0 &&
      optionalEnriched.length === 0 &&
      d.incompatible.length === 0 &&
      d.unresolvable.length === 0 &&
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
        pushSuccess(`Installed ${primaryProjectName}`);
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
  <div class="px-3 py-3 space-y-2 sticky top-0 z-10 bg-base border-b border-border-subtle">
    <div class="flex gap-2 items-center">
      <input
        type="search"
        placeholder="Search mods..."
        aria-label="Search mods"
        class="filter-control flex-1"
        oninput={onQueryInput}
      />
      <label class="text-sm text-secondary inline-flex items-center gap-1">
        Sort:
        <select bind:value={sort} class="filter-control filter-control-select">
          <option value="downloads">Downloads</option>
          <option value="relevance">Relevance</option>
          <option value="updated">Updated</option>
        </select>
      </label>
    </div>
    <div class="flex gap-3 items-center text-sm">
      <span class="text-secondary">Filters:</span>
      <label class="inline-flex items-center gap-1">
        MC:
        <McVersionCombobox bind:value={mcFilter} placeholder="Any" />
      </label>
      <label class="inline-flex items-center gap-1">
        Loader:
        <select
          bind:value={loaderFilter}
          aria-label="Loader filter"
          class="filter-control filter-control-select filter-select"
          class:is-empty={!loaderFilter}
        >
          <option value="">Any</option>
          <option value="fabric">Fabric</option>
          <option value="quilt">Quilt</option>
          <option value="forge">Forge</option>
          <option value="neoforge">NeoForge</option>
        </select>
      </label>
      <button
        type="button"
        class="btn-tertiary text-xs"
        disabled={!mcFilter && !loaderFilter}
        data-testid="mod-clear-filters"
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
        data-testid="mod-page-size"
      >
        {#each PAGE_SIZES as n}
          <option value={n}>{n} / page</option>
        {/each}
      </select>
      <label class="inline-flex items-center gap-1 ml-auto">
        <input type="checkbox" checked={showInstalled} onchange={onShowInstalledChange} />
        Show installed
      </label>
    </div>
  </div>

  <div class="p-3 space-y-2">
    {#if error}
      <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2">{error}</div>
    {/if}
    {#if loading}
      <div class="text-placeholder text-sm py-8 text-center">Searching…</div>
    {:else if pageHits.length > 0}
      {#each pageHits as hit (`${hit.source}:${hit.project_id}`)}
        <ModCard
          summary={hit}
          installed={installedFor(hit)}
          onInstall={() => startInstall(hit)}
          onOpenDetail={() => (drawerProject = hit.project_id)}
          onToggle={() => toggleCard(hit)}
          onUninstall={() => uninstallCard(hit)}
        />
      {/each}
      <div class="flex items-center justify-center gap-3 text-sm text-secondary pt-2">
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
          const depCount = prompt.required.length + chosenOptional.length;
          pushSuccess(
            depCount > 0
              ? `Installed ${prompt.primaryProjectName} + ${depCount} ${depCount === 1 ? 'dependency' : 'dependencies'}`
              : `Installed ${prompt.primaryProjectName}`,
          );
          await refreshInstalled();
        }
      }}
    />
  {/if}
{/if}
