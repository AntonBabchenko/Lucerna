<script lang="ts">
  import {
    commands,
    events,
    type Error as IpcError,
    type InstalledAsset,
    type InstalledMod,
    type LoaderKind,
    type ModSort,
    type ModSource,
    type ModSummary,
    type ModVersion,
  } from '$lib/ipc/bindings';
  import { onDestroy, untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { formatError } from '$lib/ipc/format-error';
  import { listenUntilDestroyed } from '$lib/ipc/listen';
  import { debounceTrailing } from '$lib/ui/debounce';
  import { displayLoader } from '$lib/instances/loader-display';
  import {
    formatLoaderLatestList,
    latestSupportedPerLoader,
  } from '$lib/mods/latest-supported-version';
  import type { UnresolvableDetail } from '$lib/mods/unresolvable-detail';
  import { decideModInstall, type DepItem, type OptionalItem } from '$lib/mods/dep-prompt';
  import { buildInstalledDepLines } from '$lib/mods/install-summary';
  import type { IconName } from '$lib/ui/icons';
  import { modProjectUrl } from '$lib/mods/project-url';
  import { nameKey } from '$lib/mods/name-match';
  import { prioritizeByTitle } from '$lib/mods/search-rank';
  import { t } from '$lib/i18n';
  import { get } from 'svelte/store';
  import { browserPrefs } from './browser-prefs.svelte';
  import { canInstallContent, type InstanceContentKind } from './content-kind';
  import { installFailureToast } from '$lib/mods/install-failure';
  import { pushActionToast, pushSuccess } from '$lib/toasts/toasts.svelte';
  import {
    assetsChanged,
    cfKeyVersion,
    mcVersions,
    modBrowseOpenProject,
    settingsOpen,
  } from '$lib/settings/state.svelte';
  import CurseForgeKeyBanner from './CurseForgeKeyBanner.svelte';
  import DependencyDialog from './DependencyDialog.svelte';
  import FindAlternativeDialog from './FindAlternativeDialog.svelte';
  import PageSizePicker from './PageSizePicker.svelte';
  import ModDetailModal from './ModDetailModal.svelte';
  import ModResultsGrid from './ModResultsGrid.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import Pagination from '$lib/ui/Pagination.svelte';
  import BrowseFilterBar from '$lib/browse/BrowseFilterBar.svelte';
  import { activeCount } from '$lib/browse/filter-model';

  // The Browse pane inside ModBrowserTab. Responsibilities:
  //   - Render the shared filter toolbar (search 300ms debounced, sort) with
  //     all facets inline (loader, MC version, Show installed) — no drawer.
  //     Loader + MC pre-fill from the active instance when one is selected.
  //   - Page through results with the shared Pagination control (First / Prev /
  //     Next / Last over the server total).
  //   - Detect when source = CurseForge and no API key is stored, and
  //     swap the whole search UI for CurseForgeKeyBanner — both at
  //     mount (via mods_get_curseforge_key_status) and on demand if a
  //     search returns ModsPlatformAuth.
  //   - Drive the install flow: fetch versions → resolve deps. If no
  //     deps surface, install directly. Otherwise open DependencyDialog.
  //
  // ModDetailModal and DependencyDialog read `drawerProject` and `depPrompt`
  // respectively; their state lives here so this file is the single source
  // of truth for those flows.

  let {
    source,
    instanceId,
    instanceName = null,
    mcVersion,
    loader,
    kind = 'mod',
    seedQuery = null,
    onSeedConsumed = () => {},
  }: {
    source: ModSource;
    instanceId: string | null;
    // Profile/instance name — informational, used only to make the
    // no-compatible-version error message concrete. Optional so the many
    // component tests that render ModBrowseView directly need not pass it.
    instanceName?: string | null;
    mcVersion: string | null;
    loader: LoaderKind | null;
    // Which content kind this browser is for. 'mod' keeps the historical
    // behaviour (loader facet + dependency-aware install). Resource packs
    // and shaders have no loader facet and install via assetInstall.
    // Instance-side kinds only — plugins browse through the server surfaces.
    kind?: InstanceContentKind;
    // Cross-view seed: when the Installed pre-flight panel hands off a missing
    // dependency it couldn't auto-install, the Add-ons shell sets this to the
    // dep id and flips to Browse. We seed the search box once and run the
    // search, then call onSeedConsumed so the shell clears the rune (one-shot).
    seedQuery?: string | null;
    onSeedConsumed?: () => void;
  } = $props();

  // Resource packs and shaders are loader-agnostic: Modrinth's mod loader
  // facet (fabric/forge/…) doesn't apply, and LoaderKind can't represent
  // the shader-specific facets (iris/optifine/canvas). For both non-mod
  // kinds we omit the loader filter entirely and never send a loader facet.
  const isMod = $derived(kind === 'mod');
  // Placeholder avatar icon for hits with no icon_url — kind-specific so a
  // resource pack / shader doesn't render the mod (puzzle) glyph.
  const placeholderIcon = $derived<IconName>(
    kind === 'resource_pack' ? 'resourcePack' : kind === 'shader' ? 'shader' : 'puzzle',
  );

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
  // Show everything by default; installed mods get the "Installed · vX" badge —
  // the same mark-don't-remove behaviour as Modrinth/Prism/CurseForge browsers.
  // Unchecking the inline "Show installed" toggle hides installed mods locally
  // on the current page only. Pagination always runs on the server total, so
  // when hiding is on a page may render fewer than pageSize cards. See
  // docs/superpowers/specs/2026-06-04-unified-pagination-design.md.
  let showInstalled = $state(true);
  const pageSize = $derived(browserPrefs.pageSize);

  // Offset pagination over the server-reported total: `page` is 0-based and
  // First/Prev/Next/Last map straight to a server offset = page * pageSize.
  // No accumulating buffer — each page is a single request (random-access).
  let page = $state(0);
  let hits = $state<ModSummary[]>([]);
  let total = $state(0);
  let error = $state<string | null>(null);
  let loading = $state(false);
  // Which cards' install/asset flows are currently running, keyed by project_id.
  // A Set (not a single id) because the UI allows starting an install on card B
  // while card A is still installing — a scalar would let B's start clobber A's
  // busy state and prematurely clear A's spinner.
  const installingProjectIds = new SvelteSet<string>();
  // The version_id whose install was started from the detail drawer. Threaded
  // into ModDetailModal so that exact version row / recommended CTA shows a
  // busy spinner while its install is in flight. Cleared in the install finally.
  let installingVersionId = $state<string | null>(null);
  // A card is "busy" while its install runs OR while its dependency dialog is
  // open — without the latter, the card would briefly re-enable under the
  // open dialog (the install flow's finally removes it from installingProjectIds
  // when it hands off to the dialog; onConfirm re-adds it).
  function isCardBusy(projectId: string): boolean {
    return installingProjectIds.has(projectId) || depPrompt?.primary.project_id === projectId;
  }
  let drawerProject = $state<string | null>(null);
  // When set, opens the in-app "find this mod in another source" dialog.
  // Only used for CurseForge distribution blocks (Modrinth blocks keep the
  // direct open-project-page action — there's no alternative source to offer).
  let findAlt = $state<{
    modName: string;
    curseForgeUrl: string;
    instanceId: string;
    mcVersion: string;
    loader: LoaderKind;
  } | null>(null);
  let depPrompt = $state<{
    primary: ModVersion;
    primaryProjectName: string;
    required: DepItem[];
    optional: OptionalItem[];
    incompatible: string[];
    unresolvable: UnresolvableDetail[];
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

  // Resource packs / shaders have their own per-instance registry (assets_list).
  // Unlike mods there is no enable/disable and no cross-platform name lookup —
  // an asset is matched purely by source + project_id. We keep the raw list and
  // synthesise an InstalledMod-shaped record in `installedFor` so ModCard can
  // render the same "Installed · vX" badge + Uninstall without any card change.
  let installedAssets = $state<InstalledAsset[]>([]);

  async function refreshInstalledAssets() {
    // Assets only exist for non-mod kinds with a selected instance. Capture the
    // instance so a stale result can't overwrite the current instance's badges
    // (mirrors refreshInstalled's reqId guard).
    if (isMod) {
      installedAssets = [];
      return;
    }
    const reqId = instanceId;
    if (!reqId) {
      installedAssets = [];
      return;
    }
    const r = await commands.assetsList(reqId, kind);
    if (instanceId !== reqId || r.status !== 'ok') return;
    installedAssets = r.data;
  }

  async function refreshInstalled() {
    // Capture the instance; drop the result if the user switches mid-flight
    // so a stale list can't overwrite the current instance's installed-badge
    // state.
    const reqId = instanceId;
    if (!reqId) {
      installedMods = [];
      return;
    }
    const r = await commands.modsListInstalled(reqId);
    if (instanceId !== reqId || r.status !== 'ok') return;
    // Resolve project names in a few batched requests grouped by source
    // (mirrors installed-data.svelte.ts): the old per-mod `modsProject`
    // fan-out fired hundreds of calls per refresh and was a 429 source on
    // large instances. Manual mods (source: null) skip the lookup and keep
    // projectName: null — they only ever match via the exact-id path anyway.
    const idsBySource = new Map<ModSource, Set<string>>();
    for (const m of r.data) {
      if (m.source !== null && m.project_id !== null) {
        const set = idsBySource.get(m.source) ?? new Set<string>();
        set.add(m.project_id);
        idsBySource.set(m.source, set);
      }
    }
    const nameByKey = new Map<string, string>();
    await Promise.all(
      [...idsBySource].map(async ([src, ids]) => {
        const res = await commands.modsProjects(src, [...ids]);
        if (res.status === 'ok') {
          for (const s of res.data) nameByKey.set(`${src}:${s.project_id}`, s.name);
        }
      }),
    );
    if (instanceId !== reqId) return;
    installedMods = r.data.map((m) => ({
      installed: m,
      projectName:
        m.source !== null && m.project_id !== null
          ? (nameByKey.get(`${m.source}:${m.project_id}`) ?? null)
          : null,
    }));
    // pageHits derives from installedMods via installedFor, so the current
    // page's badges (and the optional local hide-installed filter) update
    // reactively once this list resolves — no refetch needed.
  }

  $effect(() => {
    // biome-ignore lint/correctness/noUnusedVariables: reactive read
    const _id = instanceId;
    // biome-ignore lint/correctness/noUnusedVariables: reactive read
    const _kind = kind;
    // Also re-run when an asset is installed/uninstalled from the Installed
    // tab so the Browse "Installed · vX" badges stay in sync. This effect only
    // reads the signal — it must never bump it (the bump lives in the action
    // handlers below), or refreshInstalledAssets would loop.
    void assetsChanged.value;
    void refreshInstalled();
    // For mods this clears the asset list (isMod → []); for resource packs /
    // shaders it loads the per-instance asset registry so result cards flip to
    // the installed state. Re-runs when instance or kind changes.
    void refreshInstalledAssets();
  });

  // Deep-link: the Add-ons → Shaders loader hint opens Iris by flipping the
  // tab to the Mods segment (which re-keys this view) and setting
  // modBrowseOpenProject. This freshly-mounted mod browser consumes it and
  // opens the detail modal. Guarded by `isMod` so the about-to-unmount shader
  // browser doesn't steal the signal during the re-key, and by a source match
  // so the modal opens against the matching platform. Reset to null once read.
  $effect(() => {
    const link = modBrowseOpenProject.value;
    if (link && isMod && link.source === source) {
      drawerProject = link.projectId;
      modBrowseOpenProject.value = null;
    }
  });

  // Mods can be enabled/disabled/uninstalled from the Installed tab (a sibling
  // view kept mounted alongside this one). Listen for those events so the
  // Browse pane's "Installed / Disable / Uninstall" badges stay in sync
  // instead of going stale until a remount. Debounced: a with-deps install
  // emits one event per jar — collapse the burst into one refresh. Listener
  // registration/teardown is race-safe via listenUntilDestroyed (this view is
  // re-keyed on every kind switch, so racing unmounts are routine).
  const debouncedRefreshInstalled = debounceTrailing(() => void refreshInstalled(), 150);
  onDestroy(debouncedRefreshInstalled.cancel);
  listenUntilDestroyed([
    events.modInstalled.listen(debouncedRefreshInstalled.call),
    events.modUninstalled.listen(debouncedRefreshInstalled.call),
    events.modToggle.listen(debouncedRefreshInstalled.call),
  ]);

  // O(1) lookups for installedFor: rebuilt once per installedMods/assets
  // commit instead of two linear scans per card per render (a 100-card page
  // over 200 installed mods was ~20k `.find` steps every re-render, and mod
  // events re-render often).
  const installedById = $derived.by(() => {
    const map = new Map<string, InstalledMod>();
    for (const r of installedMods) {
      if (r.installed.source !== null && r.installed.project_id !== null) {
        map.set(`${r.installed.source}:${r.installed.project_id}`, r.installed);
      }
    }
    return map;
  });
  const installedByNameKey = $derived.by(() => {
    const map = new Map<string, InstalledMod>();
    for (const r of installedMods) {
      if (r.projectName !== null) {
        const k = nameKey(r.projectName);
        if (k !== '' && !map.has(k)) map.set(k, r.installed);
      }
    }
    return map;
  });
  const assetById = $derived.by(() => {
    const map = new Map<string, InstalledAsset>();
    for (const a of installedAssets) {
      map.set(`${a.source}:${a.project_id}`, a);
    }
    return map;
  });

  function installedFor(card: ModSummary): InstalledMod | null {
    // Resource packs / shaders: match purely by source + project_id against the
    // asset registry, then synthesise an InstalledMod-shaped record so ModCard
    // renders the green "Installed · vX" badge + Uninstall unchanged. Assets
    // have no enable/disable, no deps, and no enrichment — those fields are
    // filled with inert defaults (enabled: true so the badge reads "Installed").
    if (!isMod) {
      const a = assetById.get(`${card.source}:${card.project_id}`);
      if (!a) return null;
      return {
        filename: a.filename,
        sha1: a.sha1,
        source: a.source,
        project_id: a.project_id,
        version_id: a.version_id,
        name: a.name,
        version_number: a.version_number,
        installed_at: a.installed_at,
        enabled: true,
        requires: [],
        enrich_attempted: false,
      };
    }
    // Exact platform-and-id match first.
    const exact = installedById.get(`${card.source}:${card.project_id}`);
    if (exact) return exact;
    // Cross-platform fallback against the fetched project name. We
    // normalize both sides (strip platform suffixes, lowercase,
    // alphanumeric-only) so "Cloth Config API" matches "Cloth Config
    // API (Forge)" matches "Cloth Config Fabric Edition" matches just
    // "Cloth Config". False positives are bounded — "Sodium" vs
    // "Sodium Extra" still differ.
    const cardKey = nameKey(card.name);
    if (cardKey === '') return null;
    return installedByNameKey.get(cardKey) ?? null;
  }

  // The current server page, re-ranked so title matches come first (see
  // prioritizeByTitle), then optionally narrowed by the local hide-installed
  // filter. Hiding is a no-op by default (showInstalled=true); when off it
  // removes installed cards from THIS page only — the server total and page
  // count are unchanged, so the page may show fewer than pageSize cards.
  const pageHits = $derived.by(() => {
    const ranked = prioritizeByTitle(hits, query, (h) => h.name);
    return showInstalled ? ranked : ranked.filter((h) => installedFor(h) === null);
  });
  // Total pages over the server total; always >= 1 so the pager renders.
  const pageCount = $derived(Math.max(1, Math.ceil(total / pageSize)));

  async function uninstallCard(card: ModSummary) {
    if (!instanceId) return;
    // Resource packs / shaders uninstall via assetUninstall keyed on filename
    // (their registry has no sha1-based mod command). Match the installed asset
    // by source + project_id, then refresh the asset list so the card flips
    // back to "Install".
    if (!isMod) {
      const asset = installedAssets.find(
        (x) => x.source === card.source && x.project_id === card.project_id,
      );
      if (!asset) return;
      const r = await commands.assetUninstall(instanceId, kind, asset.filename);
      if (r.status === 'error') {
        error = formatError(r.error);
        return;
      }
      await refreshInstalledAssets();
      // Notify the Installed-assets view (no Tauri events for assets).
      assetsChanged.value++;
      return;
    }
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
    // biome-ignore lint/correctness/noUnusedVariables: reactive read
    const _ps = pageSize; // a page-size change re-runs the search at page 0
    // biome-ignore lint/correctness/noUnusedVariables: reactive read
    const _kind = kind;
    // `untrack` prevents the async work in `resetSearch` from registering
    // `page`, `hits`, etc. as dependencies of this effect, which would create
    // an update cycle (write → re-run → write).
    untrack(() => void resetSearch());
  });

  // Consume a one-shot search seed from the Add-ons shell (a missing-dependency
  // hand-off from the Installed pre-flight panel). The effect depends ONLY on
  // `seedQuery`; setting `query` + running the search + clearing the seed all
  // happen inside `untrack` so this can't re-trigger itself or loop.
  $effect(() => {
    const seed = seedQuery;
    if (seed == null) return;
    untrack(() => {
      query = seed;
      void resetSearch();
      onSeedConsumed();
    });
  });

  // Monotonic request id so an out-of-order modsSearch response can't clobber a
  // newer page. With offset paging (vs the old append-only buffer) a fast
  // Next→Next can resolve B-then-A; without this guard A would overwrite B and
  // show page-1 data under page 2. The Pagination control is also disabled while
  // loading, but the guard is the real correctness backstop.
  let reqSeq = 0;

  // Fetch the current page directly by server offset. One request per page —
  // First/Prev/Next/Last are O(1) random-access jumps over the server total.
  async function reload(): Promise<void> {
    if (needsCfKey) {
      hits = [];
      total = 0;
      return;
    }
    const seq = ++reqSeq;
    loading = true;
    error = null;
    const result = await commands.modsSearch({
      source,
      kind,
      query,
      // Empty input strings collapse to `null` — the search backend treats
      // null as "no MC / no loader facet". Clearing both fields (or hitting
      // Clear filters) is the explicit way to widen.
      mc_version: mcFilter || null,
      // Resource packs / shaders have no loader facet, so always send null for
      // non-mod kinds regardless of any stale filter value.
      loader: isMod ? ((loaderFilter || null) as LoaderKind | null) : null,
      sort,
      page_size: pageSize,
      offset: page * pageSize,
    });
    // A newer reload() superseded this one while it awaited — drop the stale
    // result (and leave `loading` for the in-flight request to clear).
    if (seq !== reqSeq) return;
    if (result.status === 'ok') {
      hits = result.data.hits;
      total = result.data.total;
    } else if (result.error.kind === 'mods_platform_auth') {
      needsCfKey = true;
      hits = [];
      total = 0;
    } else {
      error = formatError(result.error);
    }
    loading = false;
  }

  // Jump back to page 0 and reload — used when filters/search/source change.
  async function resetSearch(): Promise<void> {
    page = 0;
    await reload();
  }

  // Pagination control → clamp to a valid index and reload. Clamping guards a
  // stale Last/Next after the total shrank between fetches.
  async function goToPage(n: number): Promise<void> {
    const clamped = Math.min(Math.max(0, n), pageCount - 1);
    if (clamped === page) return;
    page = clamped;
    await reload();
  }

  // Flip the local hide-installed filter. Pure per-page view change — the
  // server query and total are unchanged, so no refetch is needed.
  function setShowInstalled(value: boolean) {
    showInstalled = value;
  }

  // Non-mod kinds have no loader facet: drop it from the chip row / badge
  // count so resource-pack / shader browsers never surface a loader chip.
  const filterFacets = $derived({
    loader: isMod ? loaderFilter : ('' as LoaderKind | ''),
    mc: mcFilter,
    showInstalled,
  });

  // The active instance's loader + MC — what Browse pre-fills for it. "Restore"
  // snaps loader + MC back to these (it deliberately leaves "Show installed"
  // alone — that's the user's view preference, not an instance property).
  const instanceLoaderFilter = $derived(loader && loader !== 'vanilla' ? loader : '');
  const instanceMcFilter = $derived(mcVersion ?? '');
  const canRestore = $derived(
    instanceId !== null && (loaderFilter !== instanceLoaderFilter || mcFilter !== instanceMcFilter),
  );

  function restoreInstanceFilters() {
    // Setting loader/mc triggers the search-reset effect (same path the drawer
    // bindings use); "Show installed" is intentionally left untouched.
    loaderFilter = instanceLoaderFilter;
    mcFilter = instanceMcFilter;
  }

  function clearAllFilters() {
    loaderFilter = '';
    mcFilter = '';
    setShowInstalled(true);
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

  // Turn an install-time IPC error into the right UI for the cases that
  // benefit from extra context, returning true when handled. Callers fall back
  // to their default (error bar / toast) when this returns false.
  function reportInstallError(
    err: IpcError,
    modName: string,
    modSource: ModSource,
    slugOrId: string,
  ): boolean {
    const tr = get(t);
    if (err.kind === 'mods_distribution_disabled') {
      // Distribution is disabled on the source platform. When it's CurseForge
      // (and we have instance context), offer an in-app "find on Modrinth" path
      // — the dialog keeps the manual CurseForge link as a fallback. A Modrinth
      // distribution block has no alternative source to offer, so keep the
      // manual link there.
      const platform = modSource === 'modrinth' ? 'Modrinth' : 'CurseForge';
      const url = modProjectUrl(modSource, slugOrId);
      if (modSource === 'curseforge' && instanceId && mcVersion && loader) {
        // Capture the instance context now (it's guaranteed non-null inside this
        // branch). TypeScript won't narrow these inside the toast's run callback,
        // and the live props could become null between the toast appearing and
        // the user clicking it — so the dialog renders from these captured values.
        const ctxInstanceId = instanceId;
        const ctxMcVersion = mcVersion;
        const ctxLoader = loader;
        pushActionToast(
          'warning',
          tr('mods.browse.distributionDisabledTitle', { mod: modName }),
          {
            label: tr('mods.findAlt.toastAction'),
            run: () => {
              findAlt = {
                modName,
                curseForgeUrl: url,
                instanceId: ctxInstanceId,
                mcVersion: ctxMcVersion,
                loader: ctxLoader,
              };
            },
          },
          [tr('mods.browse.distributionDisabledBody', { platform })],
        );
      } else {
        pushActionToast(
          'warning',
          tr('mods.browse.distributionDisabledTitle', { mod: modName }),
          {
            label: tr('mods.browse.distributionDisabledAction', { platform }),
            run: () => void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(url)),
          },
          [tr('mods.browse.distributionDisabledBody', { platform })],
        );
      }
      return true;
    }
    if (err.kind === 'mods_filename_conflict') {
      // Name the already-installed mod that owns the clashing filename, not just
      // the filename — far more actionable.
      const existing = installedMods.find((r) => r.installed.sha1 === err.existing_sha);
      if (existing) {
        error = tr('mods.browse.errorFilenameConflictNamed', {
          filename: err.filename,
          newMod: modName,
          existingMod: existing.projectName ?? existing.installed.name,
        });
        return true;
      }
    }
    return false;
  }

  // No version of the mod matches the active instance's MC + loader. Fetch the
  // mod's full version set (all loaders) and explain precisely why: either the
  // mod is for a different loader, or it just doesn't cover this MC version —
  // listing the latest MC it supports per loader.
  async function reportNoCompatibleVersion(card: ModSummary, currentLoader: LoaderKind) {
    const tr = get(t);
    const all = await commands.modsVersions(card.source, card.project_id, null, null);
    if (all.status === 'error') {
      error = tr('mods.browse.errorNoCompatibleVersion');
      return;
    }
    const perLoader = latestSupportedPerLoader(
      all.data,
      mcVersions.value.map((v) => v.id),
    );
    if (perLoader.length === 0) {
      error = tr('mods.browse.errorNoCompatibleVersion');
      return;
    }
    const list = formatLoaderLatestList(perLoader, displayLoader);
    if (perLoader.some((p) => p.loader === currentLoader)) {
      error = tr('mods.browse.errorNoVersionForMc', {
        mod: card.name,
        mcVersion: mcVersion ?? '',
        profile: instanceName ?? '',
        list,
      });
    } else {
      error = tr('mods.browse.errorWrongLoader', {
        mod: card.name,
        loader: displayLoader(currentLoader),
        profile: instanceName ?? '',
        list,
      });
    }
  }

  // Resource packs / shaders install via the asset command — no loader,
  // no dependency resolution. We still pin to a concrete ModVersion: the
  // detail modal passes one explicitly; a card "Install" picks the latest
  // version compatible with the instance's MC (no loader facet).
  async function startAssetInstall(card: ModSummary, pinnedVersion?: ModVersion) {
    // Mark this card busy across the whole flow; the finally clears it on every
    // exit path (early returns and the happy path alike).
    installingProjectIds.add(card.project_id);
    try {
      if (!instanceId || !canInstallContent(kind, instanceId, loader)) {
        error = get(t)('mods.browse.errorNoInstance');
        return;
      }
      let version: ModVersion;
      if (pinnedVersion) {
        version = pinnedVersion;
      } else {
        const versions = await commands.modsVersions(card.source, card.project_id, mcVersion, null);
        if (versions.status === 'error') {
          error = formatError(versions.error);
          return;
        }
        if (versions.data.length === 0) {
          error = get(t)('mods.browse.errorNoCompatibleVersion');
          return;
        }
        version = versions.data[0]!;
      }
      const installed = await commands.assetInstall(instanceId, version, kind);
      if (installed.status === 'error') {
        installFailureToast(card.name, installed.error, () => {
          void startAssetInstall(card, pinnedVersion);
        });
        return;
      }
      pushSuccess(get(t)('mods.browse.toastInstalledMod', { name: card.name }), []);
      // Mods refresh their installed-state via Tauri events; assets have no such
      // events, so refresh the asset list explicitly to flip the card to the
      // "Installed · vX" + Uninstall state immediately.
      await refreshInstalledAssets();
      // Notify the Installed-assets view so it re-lists.
      assetsChanged.value++;
    } finally {
      installingProjectIds.delete(card.project_id);
    }
  }

  // The dependency-dialog confirm flow, hoisted out of the markup so the
  // failure toast's Retry can re-run it with the same captured arguments.
  async function confirmDepInstall(
    prompt: NonNullable<typeof depPrompt>,
    chosenOptional: ModVersion[],
  ) {
    if (!instanceId) return;
    // Keep the originating card busy while the confirmed install runs.
    installingProjectIds.add(prompt.primary.project_id);
    try {
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
        if (
          !reportInstallError(
            installed.error,
            prompt.primaryProjectName,
            prompt.primary.source,
            prompt.primary.project_id,
          )
        ) {
          installFailureToast(prompt.primaryProjectName, installed.error, () => {
            void confirmDepInstall(prompt, chosenOptional);
          });
        }
        // The backend rolled the partial install back — re-sync the
        // installed badges with the (unchanged) on-disk state.
        await refreshInstalled();
      } else {
        // Build the per-mod toast from the dialog's already-resolved project
        // names (the backend's InstallSummary carries release titles, not mod
        // names). Lines = every newly-installed dependency: the primary's
        // requireds + each chosen optional and its transitive requireds,
        // deduped by project. Matches exactly what the dialog showed.
        const depLines = buildInstalledDepLines(prompt, chosenOptional);
        pushSuccess(
          get(t)('mods.browse.toastInstalledMod', { name: prompt.primaryProjectName }),
          depLines,
        );
        await refreshInstalled();
      }
    } finally {
      installingProjectIds.delete(prompt.primary.project_id);
    }
  }

  async function startInstall(card: ModSummary, pinnedVersion?: ModVersion) {
    // Resource packs and shaders take the asset path; mods keep the
    // dependency-aware flow below untouched. startAssetInstall sets its own
    // busy flag, so we return before flagging here.
    if (kind !== 'mod') {
      await startAssetInstall(card, pinnedVersion);
      return;
    }
    // Mark this card busy for the whole mod flow — including the branch that
    // only opens the dependency dialog. The finally clears it on every exit
    // path (early returns, the fast-path install, and the dialog-open path).
    installingProjectIds.add(card.project_id);
    try {
      if (!instanceId || !mcVersion || !loader) {
        error = get(t)('mods.browse.errorNoInstance');
        return;
      }
      if (loader === 'vanilla') {
        error = get(t)('mods.browse.errorVanillaLoader');
        return;
      }
      let primary: ModVersion;
      if (pinnedVersion) {
        // Drawer passes the user's explicit choice. Skip the lookup.
        primary = pinnedVersion;
      } else {
        const versions = await commands.modsVersions(
          card.source,
          card.project_id,
          mcVersion,
          loader,
        );
        if (versions.status === 'error') {
          error = formatError(versions.error);
          return;
        }
        if (versions.data.length === 0) {
          await reportNoCompatibleVersion(card, loader);
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
        if (!reportInstallError(plan.error, card.name, card.source, card.slug ?? card.project_id))
          error = formatError(plan.error);
        return;
      }
      const decision = await decideModInstall(plan.data, primary, {
        loader,
        mcVersion,
        manifestOrder: mcVersions.value.map((v) => v.id),
        displayLoader,
        fetchProjectName: async (s, id) => {
          const proj = await commands.modsProject(s, id);
          return proj.status === 'ok' ? proj.data.summary.name : null;
        },
        fetchVersions: async (s, id) => {
          const res = await commands.modsVersions(s, id, null, null);
          return res.status === 'ok' ? res.data : null;
        },
      });
      if (decision.kind === 'install') {
        const { primaryProjectName } = decision;
        const installed = await commands.modsInstallWithDeps(
          instanceId,
          {
            source: primary.source,
            project_id: primary.project_id,
            version_id: primary.version_id,
          },
          [],
        );
        if (installed.status === 'error') {
          if (
            !reportInstallError(
              installed.error,
              primaryProjectName,
              primary.source,
              card.slug ?? card.project_id,
            )
          ) {
            installFailureToast(primaryProjectName, installed.error, () => {
              void startInstall(card, pinnedVersion);
            });
          }
          // The backend rolled the partial install back — re-sync the
          // installed badges with the (unchanged) on-disk state.
          await refreshInstalled();
        } else {
          // Fast path has no dependencies; use the resolved project name (not
          // the backend's release-title `primary_name`) for the toast title.
          pushSuccess(get(t)('mods.browse.toastInstalledMod', { name: primaryProjectName }), []);
          await refreshInstalled();
        }
      } else {
        depPrompt = decision.prompt;
      }
    } finally {
      installingProjectIds.delete(card.project_id);
    }
  }
</script>

{#if isMod && loader === 'vanilla'}
  <div
    class="p-6 bg-warning-bg border border-warning-text/30 rounded mx-3 my-4 text-sm text-warning-text"
  >
    <div class="font-medium mb-1">{$t('mods.browse.vanillaHeading')}</div>
    <p class="text-warning-text">
      {$t('mods.browse.vanillaBody')}
    </p>
  </div>
{:else if needsCfKey}
  <CurseForgeKeyBanner onOpenSettings={() => (settingsOpen.value = { tab: 'integrations' })} />
{:else}
  <div class="sticky top-0 z-10 bg-base border-b border-border-subtle">
    <BrowseFilterBar
      searchAriaLabel={$t('mods.browse.searchAriaLabel')}
      searchPlaceholder={$t('mods.browse.searchPlaceholder')}
      {sort}
      sortOptions={[
        { value: 'downloads', label: $t('mods.browse.sortDownloads') },
        { value: 'relevance', label: $t('mods.browse.sortRelevance') },
        { value: 'updated', label: $t('mods.browse.sortUpdated') },
      ]}
      showLoader={isMod}
      bind:loader={loaderFilter}
      bind:mc={mcFilter}
      {showInstalled}
      onShowInstalledChange={(v) => setShowInstalled(v)}
      {canRestore}
      restoreLabel={$t('browse.filter.restoreForInstance')}
      onRestore={restoreInstanceFilters}
      activeCount={activeCount(filterFacets)}
      onClearAll={clearAllFilters}
      {onSearchInput}
      onSortChange={(v) => (sort = v as ModSort)}
    />
  </div>

  <div class="p-3 space-y-2">
    {#if error}
      <div
        class="bg-danger-bg border border-danger text-danger text-sm rounded p-2 flex items-center justify-between gap-3"
      >
        <span>{error}</span>
        <button type="button" class="btn-secondary btn-sm shrink-0" onclick={() => void reload()}>
          {$t('mods.browse.errorRetry')}
        </button>
      </div>
    {/if}
    {#if loading}
      <LoadingPanel label={$t('mods.browse.searching')} delayMs={0} />
    {:else if hits.length > 0}
      {#if pageHits.length > 0}
        <ModResultsGrid
          hits={pageHits}
          layout={browserPrefs.layout === 'grid' ? 'grid' : 'list'}
          {isMod}
          {placeholderIcon}
          {installedFor}
          {isCardBusy}
          onInstall={(hit) => startInstall(hit)}
          onOpenDetail={(hit) => (drawerProject = hit.project_id)}
          onToggle={(hit) => toggleCard(hit)}
          onUninstall={(hit) => uninstallCard(hit)}
        />
      {:else}
        <!-- Hide-installed removed every card on this page. The page still
             exists in the server total, so keep the pager available rather than
             dead-ending on "No results". -->
        <div class="text-placeholder text-sm py-8 text-center">
          {$t('mods.browse.allInstalledOnPage')}
        </div>
      {/if}
      <!-- Steam-style footer: shared pagination control, per-page selector right. -->
      <Pagination {page} {pageCount} disabled={loading} onPage={(n) => void goToPage(n)}>
        {#snippet end()}
          <PageSizePicker />
        {/snippet}
      </Pagination>
    {:else}
      <div class="text-placeholder text-sm py-8 text-center">{$t('mods.browse.noResults')}</div>
    {/if}
  </div>

  {#if drawerProject}
    <ModDetailModal
      {source}
      projectId={drawerProject}
      {mcVersion}
      loader={isMod ? loader : null}
      {kind}
      installedVersionId={installedMods.find(
        (r) => r.installed.source === source && r.installed.project_id === drawerProject,
      )?.installed.version_id ?? null}
      {installingVersionId}
      onClose={() => (drawerProject = null)}
      onInstall={(v) => {
        // Drawer passes the explicit version the user picked. We
        // re-use startInstall — it now accepts a pinnedVersion arg so
        // we skip the latest-version lookup and install exactly what
        // the user clicked. If a different version is already
        // installed for this project, startInstall handles the swap.
        //
        // Keep the drawer open while the install runs so the chosen version's
        // row / recommended CTA shows its busy spinner; close it when the
        // install settles (success or error) so the browse list — and any
        // install-error banner — is visible again underneath.
        installingVersionId = v.version_id;
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
        ).finally(() => {
          installingVersionId = null;
          drawerProject = null;
        });
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
      onConfirm={(chosenOptional) => {
        const prompt = depPrompt;
        depPrompt = null;
        if (prompt) void confirmDepInstall(prompt, chosenOptional);
      }}
    />
  {/if}
  {#if findAlt}
    <FindAlternativeDialog
      modName={findAlt.modName}
      mcVersion={findAlt.mcVersion}
      loader={findAlt.loader}
      instanceId={findAlt.instanceId}
      curseForgeUrl={findAlt.curseForgeUrl}
      onClose={() => (findAlt = null)}
    />
  {/if}
{/if}
