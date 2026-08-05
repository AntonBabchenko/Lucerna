<script lang="ts">
  import {
    commands,
    type InstalledMod,
    type InstallMissingReport,
    type ModSort,
    type ModSource,
    type ModSummary,
    type ModVersion,
    type ServerDatapackEntry,
  } from '$lib/ipc/bindings';
  import { SvelteSet } from 'svelte/reactivity';
  import { get } from 'svelte/store';
  import { t } from '$lib/i18n';
  import { formatError } from '$lib/ipc/format-error';
  import { modProjectUrl } from '$lib/mods/project-url';
  import { browserPrefs } from '$lib/mods/browser-prefs.svelte';
  import { cfKeyVersion, settingsOpen } from '$lib/settings/state.svelte';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import ModResultsGrid from '$lib/mods/ModResultsGrid.svelte';
  import CurseForgeKeyBanner from '$lib/mods/CurseForgeKeyBanner.svelte';
  import SourcePicker from '$lib/mods/SourcePicker.svelte';
  import LayoutToggle from '$lib/mods/LayoutToggle.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Pagination from '$lib/ui/Pagination.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import ServerContentDetail from '$lib/servers/browser/ServerContentDetail.svelte';

  // A datapack-targeted server catalog browser (Task 12). Copied from
  // ServerModBrowser and keeps its machinery — pagination, the reqSeq
  // out-of-order guard, needsCfKey handling, the results grid, the detail
  // modal, the source-picker plumbing — diverging only where a datapack
  // genuinely differs from a mod:
  //   - no loader prop/facet: a datapack has no loader.
  //   - version listing is modsDatapackVersions, never modsVersions — a
  //     hybrid project (mod jar + datapack zip under one listing, e.g.
  //     Terralith) would otherwise return the jar.
  //   - install is serverInstallDatapackVersion, which differs from
  //     serverInstallMod/serverInstallPlugin in BOTH directions: it takes the
  //     full ModVersion (not just a version id) and returns a
  //     ServerInstalledRecord (not an InstallMissingReport). ServerContentDetail's
  //     installVersion/loadVersions props are hard-typed to the mod/plugin
  //     install envelope, so rather than widen that shared component's
  //     contract (touching the two OTHER panes that use it), this component
  //     adapts locally: the detail modal's version fetch caches the list by
  //     version_id, and install looks the object up there. A datapack install
  //     has no dependency graph, so { installed: [filename], unresolved: [] }
  //     is a truthful — not fudged — InstallMissingReport.
  //   - the installed badge stays the ordinary "Installed" (never "In
  //     library"): that client-only wording exists because a library pack is
  //     inert until placed in a world, but a server has one world and a
  //     present-and-unlisted pack auto-enables — it IS live. ModCard already
  //     renders the ordinary badge as long as this component doesn't reach for
  //     the client's installedLabelFor override, so there is nothing extra to
  //     do here.

  let {
    serverId,
    mcVersion,
    onInstalled,
    // Bindable so the Add-ons host can own the source picker in its sub-tab
    // row (showSourcePicker={false}) while legacy embedders keep the inline
    // picker with the local default.
    source = $bindable<ModSource>('modrinth'),
    showSourcePicker = true,
  }: {
    serverId: string;
    mcVersion: string;
    onInstalled: () => void;
    source?: ModSource;
    showSourcePicker?: boolean;
  } = $props();

  let sort = $state<ModSort>('downloads');
  let query = $state('');
  let page = $state(0);
  let hits = $state<ModSummary[]>([]);
  let total = $state(0);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let needsCfKey = $state(false);
  // Projects whose install is in flight, keyed by project_id (a Set so two
  // installs can overlap without clobbering each other's busy state).
  const installing = new SvelteSet<string>();
  // The project whose in-launcher detail card is open (null = closed).
  let detail = $state<ModSummary | null>(null);
  // Client parity (ModBrowseView): default shows installed cards (with their
  // "Installed" badge); unchecking hides them so only not-yet-installed packs
  // remain on the current page. Pure client-side filter — no refetch.
  let showInstalled = $state(true);

  const pageSize = $derived(browserPrefs.pageSize);
  const pageCount = $derived(Math.max(1, Math.ceil(total / pageSize)));

  const sortOptions = $derived([
    { value: 'downloads', label: $t('mods.browse.sortDownloads') },
    { value: 'relevance', label: $t('mods.browse.sortRelevance') },
    { value: 'updated', label: $t('mods.browse.sortUpdated') },
  ]);

  let reqSeq = 0;

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
      kind: 'datapack',
      query,
      mc_version: mcVersion,
      loader: null,
      sort,
      page_size: pageSize,
      offset: page * pageSize,
    });
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

  async function refreshCfStatus(): Promise<void> {
    if (source !== 'curseforge') {
      needsCfKey = false;
      return;
    }
    const s = await commands.modsGetCurseforgeKeyStatus();
    needsCfKey = s.status === 'ok' ? s.data === 'missing' : true;
  }

  // Single source of truth for loading: the first run loads immediately (no
  // debounce delay on open); subsequent query/source/sort/cf-key changes
  // debounce. Driving everything from one $effect avoids the onMount + effect
  // double-fire that would issue two searches on every mount.
  let debounce: ReturnType<typeof setTimeout> | null = null;
  let firstRun = true;
  $effect(() => {
    const _ = query;
    const __ = source;
    const ___ = cfKeyVersion.value;
    const ____ = sort;
    if (debounce) clearTimeout(debounce);
    if (firstRun) {
      firstRun = false;
      void (async () => {
        await refreshCfStatus();
        await reload();
      })();
      return;
    }
    debounce = setTimeout(() => {
      void (async () => {
        await refreshCfStatus();
        page = 0;
        await reload();
      })();
    }, 250);
    return () => {
      if (debounce) clearTimeout(debounce);
    };
  });

  function onPage(n: number): void {
    page = n;
    void reload();
  }

  // Shared success/dependency/unresolved toast block, mirroring ServerModBrowser.
  // A datapack install carries no dependency graph, so `report.unresolved` is
  // always empty in practice — the block is kept as-is so both callers stay
  // structurally identical.
  function toastInstalled(name: string, report: InstallMissingReport): void {
    const depCount = Math.max(0, report.installed.length - 1);
    const msg =
      depCount > 0
        ? get(t)('servers.mods.installedWithDeps', { name, count: depCount })
        : get(t)('servers.mods.installedOne', { name });
    pushSuccess(msg);
    if (report.unresolved.length > 0) {
      pushWarning(
        get(t)('servers.mods.someDepsUnresolved', { deps: report.unresolved.join(', ') }),
      );
    }
    onInstalled();
  }

  async function install(card: ModSummary): Promise<void> {
    installing.add(card.project_id);
    error = null;
    try {
      const versions = await commands.modsDatapackVersions(card.source, card.project_id, mcVersion);
      if (versions.status !== 'ok') {
        error = formatError(versions.error);
        return;
      }
      if (versions.data.length === 0) {
        pushWarning(get(t)('servers.mods.noCompatibleVersion', { name: card.name }));
        return;
      }
      // modsDatapackVersions returns newest-first (same assumption modsVersions'
      // callers make).
      const newest = versions.data[0];
      if (!newest.primary_file.distribution_allowed) {
        // CurseForge "author disabled third-party downloads": never fetched
        // in-app. Open the project page and point the user at the local-install path.
        openExternalPage(card);
        pushWarning(get(t)('servers.mods.externalDownload', { name: card.name }));
        return;
      }
      const res = await commands.serverInstallDatapackVersion(serverId, newest);
      if (res.status === 'ok') {
        toastInstalled(card.name, { installed: [res.data.filename], unresolved: [] });
        // Flip the just-installed card to its "Installed" state immediately.
        await loadInstalled();
      } else {
        error = formatError(res.error);
      }
    } finally {
      installing.delete(card.project_id);
    }
  }

  // ── Installed-state parity (client ModBrowseView) ──────────────────────────
  // Map of installed identities for the mounted server, keyed
  // `${source}:${project_id}`, so a browse card renders the "Installed" state
  // (toggle + uninstall) instead of a re-installable button. Reassigned
  // immutably (new Map) after every mutation; ModResultsGrid calls
  // installedFor(hit) in its render, so swapping the map re-renders the cards.
  let installedByKey = $state(new Map<string, ServerDatapackEntry>());

  async function loadInstalled(): Promise<void> {
    const res = await commands.serverListDatapacks(serverId);
    if (res.status !== 'ok') return; // best-effort; leave the map as-is
    const m = new Map<string, ServerDatapackEntry>();
    for (const e of res.data) {
      if (e.record.source && e.record.project_id) {
        m.set(`${e.record.source}:${e.record.project_id}`, e);
      }
    }
    installedByKey = m;
  }

  // serverId is fixed per mount; load once (and re-run if it ever changes).
  $effect(() => {
    void serverId;
    void loadInstalled();
  });

  function installedFor(card: ModSummary): InstalledMod | null {
    const e = installedByKey.get(`${card.source}:${card.project_id}`);
    if (!e) return null;
    return {
      filename: e.record.filename,
      sha1: e.record.sha1,
      source: e.record.source,
      project_id: e.record.project_id,
      version_id: e.record.version_id,
      name: e.record.name ?? card.name,
      version_number: e.record.version_number,
      installed_at: '',
      enabled: e.state === 'enabled',
      requires: [],
      enrich_attempted: false,
    };
  }

  // Current-page cards after the "Show installed" filter. When the toggle is
  // off, already-installed cards drop out of the rendered grid (server total /
  // page-count are unchanged, so a page may render fewer cards — matches the
  // documented client behavior). Empty-state checks stay keyed on the original
  // `hits` so a fully-installed page renders an empty grid, not "no results".
  const visibleHits = $derived(showInstalled ? hits : hits.filter((h) => installedFor(h) === null));

  // Enable/disable an installed browse card. Backend refuses while the server
  // runs (surfaces via `error`), consistent with install already behaving that
  // way on this tab — no extra gating needed.
  async function toggleInstalled(card: ModSummary): Promise<void> {
    const e = installedByKey.get(`${card.source}:${card.project_id}`);
    if (!e) return;
    const res = await commands.serverSetDatapackEnabled(
      serverId,
      e.record.filename,
      e.state !== 'enabled',
    );
    if (res.status === 'ok') {
      await loadInstalled();
      onInstalled();
    } else {
      error = formatError(res.error);
    }
  }

  async function uninstallInstalled(card: ModSummary): Promise<void> {
    const e = installedByKey.get(`${card.source}:${card.project_id}`);
    if (!e) return;
    const res = await commands.serverRemoveDatapack(serverId, e.record.filename);
    if (res.status === 'ok') {
      await loadInstalled();
      onInstalled();
    } else {
      error = formatError(res.error);
    }
  }

  function openUrl(url: string): void {
    void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(url));
  }

  function openExternalPage(card: ModSummary): void {
    openUrl(modProjectUrl(card.source, card.slug ?? card.project_id, card.author));
  }

  // For the detail modal: a version whose file is not distributable must open
  // the project page, never download.
  function externalOf(card: ModSummary, v: ModVersion): string | null {
    if (v.primary_file.distribution_allowed) return null;
    return modProjectUrl(card.source, card.slug ?? card.project_id, card.author);
  }

  // ── Detail-modal install adapter ────────────────────────────────────────
  // See the top-of-file note: serverInstallDatapackVersion needs the full
  // ModVersion and returns a ServerInstalledRecord, neither of which matches
  // ServerContentDetail's mod/plugin-shaped installVersion/loadVersions props.
  // These two local types mirror ServerContentDetail's own (private) aliases
  // so the adapter functions below type-check against the same envelope shape
  // without exporting anything from that shared component.
  type VersionsResult = Awaited<ReturnType<typeof commands.modsVersions>>;
  type InstallResult = Awaited<ReturnType<typeof commands.serverInstallMod>>;

  // Populated as a side effect of the SAME fetch ServerContentDetail performs
  // via loadVersions, so it is never stale while the modal is open — a fresh
  // loadVersions call (new project, or the modal reopened) always happens
  // before any install the modal can trigger.
  let detailVersionsCache = new Map<string, ModVersion>();

  async function loadDetailVersions(projectSource: ModSource, projectId: string): Promise<VersionsResult> {
    const res = await commands.modsDatapackVersions(projectSource, projectId, mcVersion);
    if (res.status === 'ok') {
      detailVersionsCache = new Map(res.data.map((v) => [v.version_id, v]));
    }
    return res;
  }

  async function installDetailVersion(versionId: string): Promise<InstallResult> {
    const v = detailVersionsCache.get(versionId);
    if (!v) {
      // Defensive only — the cache is populated by the same loadVersions call
      // the modal always makes before offering any version to install.
      return { status: 'error', error: { kind: 'server_content_stale' } };
    }
    const res = await commands.serverInstallDatapackVersion(serverId, v);
    if (res.status === 'error') return res;
    return { status: 'ok', data: { installed: [res.data.filename], unresolved: [] } };
  }
</script>

<div class="flex flex-col gap-2" data-testid="server-datapack-browser">
  <!-- Toolbar: source + search + pinned facets (Minecraft version only — a
       datapack has no loader) + sort + layout -->
  <div class="flex items-center gap-2">
    {#if showSourcePicker}
      <SourcePicker value={source} onChange={(v) => (source = v)} />
    {/if}
    <input
      type="search"
      class="filter-control flex-1 min-w-[8rem]"
      placeholder={$t('servers.mods.searchPlaceholder')}
      aria-label={$t('servers.mods.searchPlaceholder')}
      bind:value={query}
      data-testid="server-datapack-search"
    />
    <span class="text-xs text-muted whitespace-nowrap"
      >{$t('servers.datapacks.pinnedFacets', { mcVersion })}</span
    >
    <Select
      class="filter-control filter-control-select"
      value={sort}
      options={sortOptions}
      onChange={(v) => (sort = v as ModSort)}
      ariaLabel={$t('browse.filter.sortLabel')}
      dataTestid="server-datapack-sort"
    />
    <label class="flex shrink-0 items-center gap-1.5 text-xs text-secondary whitespace-nowrap">
      <input
        type="checkbox"
        class="accent-accent"
        bind:checked={showInstalled}
        data-testid="server-datapack-show-installed"
      />
      {$t('browse.filter.showInstalled')}
    </label>
    <LayoutToggle />
  </div>

  {#if needsCfKey}
    <CurseForgeKeyBanner onOpenSettings={() => (settingsOpen.value = { tab: 'integrations' })} />
  {:else if error}
    <p class="text-sm text-danger">{error}</p>
  {:else if loading && hits.length === 0}
    <LoadingPanel label={$t('servers.mods.searching')} delayMs={0} />
  {:else if hits.length === 0}
    <p class="py-6 text-center text-sm text-muted">{$t('servers.mods.noResults')}</p>
  {:else}
    <ModResultsGrid
      hits={visibleHits}
      layout={browserPrefs.layout}
      isMod={true}
      placeholderIcon="world"
      {installedFor}
      isCardBusy={(id) => installing.has(id)}
      onInstall={(h) => void install(h)}
      onOpenDetail={(h) => (detail = h)}
      onToggle={(h) => void toggleInstalled(h)}
      onUninstall={(h) => void uninstallInstalled(h)}
    />
    <div class="sticky bottom-0 z-10 bg-base border-t border-border-subtle">
      <Pagination {page} {pageCount} disabled={loading} {onPage} />
    </div>
  {/if}
</div>

{#if detail}
  {@const d = detail}
  <ServerContentDetail
    project={d}
    onClose={() => (detail = null)}
    loadProject={() => commands.modsProject(d.source, d.project_id)}
    loadVersions={() => loadDetailVersions(d.source, d.project_id)}
    installVersion={(vid) => installDetailVersion(vid)}
    externalOf={(v) => externalOf(d, v)}
    openExternal={openUrl}
    projectUrl={modProjectUrl(d.source, d.slug ?? d.project_id, d.author)}
    onInstalled={(report) => {
      // Toast copy keys on the PROJECT name ("Installed WorldEdit"); the
      // version label the modal passes is not surfaced in the toast.
      toastInstalled(d.name, report);
      // Reflect the install in the browse cards behind the modal.
      void loadInstalled();
    }}
  />
{/if}
