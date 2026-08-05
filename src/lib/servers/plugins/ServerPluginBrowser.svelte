<script lang="ts">
  import { untrack } from 'svelte';
  import {
    commands,
    type InstalledMod,
    type InstallMissingReport,
    type ModSort,
    type ModSource,
    type ModSummary,
    type ModVersion,
    type ServerCore,
    type ServerPluginEntryEnriched,
  } from '$lib/ipc/bindings';
  import { SvelteSet } from 'svelte/reactivity';
  import { get } from 'svelte/store';
  import { t } from '$lib/i18n';
  import { formatError } from '$lib/ipc/format-error';
  import { modProjectUrl } from '$lib/mods/project-url';
  import { browserPrefs } from '$lib/mods/browser-prefs.svelte';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import ModResultsGrid from '$lib/mods/ModResultsGrid.svelte';
  import SourcePicker from '$lib/mods/SourcePicker.svelte';
  import LayoutToggle from '$lib/mods/LayoutToggle.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Pagination from '$lib/ui/Pagination.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import ServerContentDetail from '$lib/servers/browser/ServerContentDetail.svelte';
  import { displayCore } from '$lib/servers/core-display';

  // The plugin-flavoured sibling of ServerModBrowser: same seq-guarded
  // debounced reload effect, pagination, and result grid, but targets the
  // plugin kind (Modrinth + Hangar only — neither needs an API key, so there
  // is no CurseForge-key banner here) and installs via serverInstallPlugin.
  // Hangar projects can be externally hosted (distribution_allowed=false);
  // those are never fetched in-app — we open the project page instead and
  // point the user at "Install .jar".

  let {
    serverId,
    mcVersion,
    core,
    onInstalled,
    // Bindable so an Add-ons host can own the source picker in its sub-tab
    // row (showSourcePicker={false}) while legacy embedders keep the inline
    // picker with the local default.
    source = $bindable<ModSource>('modrinth'),
    showSourcePicker = true,
  }: {
    serverId: string;
    mcVersion: string;
    core: ServerCore;
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
  // Projects whose install is in flight, keyed by project_id (a Set so two
  // installs can overlap without clobbering each other's busy state).
  const installing = new SvelteSet<string>();
  // The project whose in-launcher detail card is open (null = closed).
  let detail = $state<ModSummary | null>(null);
  // Client parity (ModBrowseView): default shows installed cards (with their
  // "Installed" badge); unchecking hides them so only not-yet-installed plugins
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
    const seq = ++reqSeq;
    loading = true;
    error = null;
    const result = await commands.modsSearch({
      source,
      kind: 'plugin',
      query,
      mc_version: mcVersion,
      loader: null,
      plugin_core: core,
      sort,
      page_size: pageSize,
      offset: page * pageSize,
    });
    if (seq !== reqSeq) return;
    if (result.status === 'ok') {
      hits = result.data.hits;
      total = result.data.total;
    } else {
      error = formatError(result.error);
    }
    loading = false;
  }

  // Single source of truth for loading: the first run loads immediately (no
  // debounce delay on open); subsequent query/source/sort changes debounce.
  // Driving everything from one $effect avoids the onMount + effect double-fire
  // that would issue two searches on every mount.
  //
  // Tracked-deps contract: this effect deliberately depends on query + source
  // + sort ONLY (the three explicit reads below). Everything reload() touches
  // before its first await — page, pageSize, mcVersion, core — must stay
  // untracked: page changes fetch explicitly via onPage (a tracked `page`
  // would make the onPage write re-fire this effect and debounce a page=0
  // reload, snapping the user back to page 1), and core/mcVersion are fixed
  // for the mounted server. The synchronous first-run call is therefore
  // wrapped in untrack(); the debounced path runs in a timeout callback,
  // which Svelte never tracks, but gets the same wrapper so the contract
  // holds by construction rather than by accident of scheduling.
  let debounce: ReturnType<typeof setTimeout> | null = null;
  let firstRun = true;
  $effect(() => {
    const _ = query;
    const __ = source;
    const ___ = sort;
    if (debounce) clearTimeout(debounce);
    if (firstRun) {
      firstRun = false;
      untrack(() => void reload());
      return;
    }
    debounce = setTimeout(() => {
      untrack(() => {
        page = 0;
        void reload();
      });
    }, 250);
    return () => {
      if (debounce) clearTimeout(debounce);
    };
  });

  function onPage(n: number): void {
    page = n;
    void reload();
  }

  function openExternalPage(url: string, card: ModSummary): void {
    const target =
      url.length > 0 ? url : modProjectUrl(card.source, card.slug ?? card.project_id, card.author);
    void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(target));
  }

  // Shared success/dependency/unresolved toast block. Both the card-install
  // (install newest) and the detail-modal install (install chosen version)
  // land here, so the toast copy lives in exactly one place (DRY).
  function toastInstalled(name: string, report: InstallMissingReport): void {
    const depCount = Math.max(0, report.installed.length - 1);
    const msg =
      depCount > 0
        ? get(t)('servers.plugins.installedWithDeps', { name, count: depCount })
        : get(t)('servers.plugins.installedOne', { name });
    pushSuccess(msg);
    if (report.unresolved.length > 0) {
      pushWarning(
        get(t)('servers.plugins.someDepsUnresolved', { deps: report.unresolved.join(', ') }),
      );
    }
    onInstalled();
  }

  async function install(card: ModSummary): Promise<void> {
    installing.add(card.project_id);
    error = null;
    try {
      const versions = await commands.modsPluginVersions(
        card.source,
        card.project_id,
        mcVersion,
        core,
      );
      if (versions.status !== 'ok') {
        error = formatError(versions.error);
        return;
      }
      if (versions.data.length === 0) {
        pushWarning(get(t)('servers.plugins.noCompatibleVersion', { name: card.name }));
        return;
      }
      const newest = versions.data[0];
      if (!newest.primary_file.distribution_allowed) {
        // Externally hosted (Hangar externalUrl): never downloaded in-app.
        // Open the page and point the user at the local-install path.
        openExternalPage(newest.primary_file.url, card);
        pushWarning(get(t)('servers.plugins.externalDownload', { name: card.name }));
        return;
      }
      const res = await commands.serverInstallPlugin(
        serverId,
        card.source,
        card.project_id,
        newest.version_id,
      );
      if (res.status === 'ok') {
        toastInstalled(card.name, res.data);
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
  // (toggle + uninstall) instead of a re-installable button — this is what stops
  // the same plugin being installed over and over (toast spam). Reassigned
  // immutably (new Map) after every mutation; ModResultsGrid calls
  // installedFor(hit) in its render, so swapping the map re-renders the cards.
  let installedByKey = $state(new Map<string, ServerPluginEntryEnriched>());

  async function loadInstalled(): Promise<void> {
    const res = await commands.serverListPluginsEnriched(serverId);
    if (res.status !== 'ok') return; // best-effort; leave the map as-is
    const m = new Map<string, ServerPluginEntryEnriched>();
    for (const e of res.data) if (e.source && e.project_id) m.set(`${e.source}:${e.project_id}`, e);
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
      filename: e.filename,
      sha1: e.sha1,
      source: e.source,
      project_id: e.project_id,
      version_id: e.version_id,
      name: e.name ?? card.name,
      version_number: e.version_number,
      installed_at: '',
      enabled: !e.disabled,
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

  // Enable/disable an installed browse card. Mutations join on_disk_filename
  // (base filename + `.disabled` when disabled), never the base filename.
  // Backend refuses while the server runs (surfaces via `error`), consistent
  // with how install already behaves on this tab — no extra gating needed.
  async function toggleInstalled(card: ModSummary): Promise<void> {
    const e = installedByKey.get(`${card.source}:${card.project_id}`);
    if (!e) return;
    const res = e.disabled
      ? await commands.serverEnablePlugin(serverId, e.on_disk_filename)
      : await commands.serverDisablePlugin(serverId, e.on_disk_filename);
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
    const res = await commands.serverDeletePlugin(serverId, e.on_disk_filename);
    if (res.status === 'ok') {
      await loadInstalled();
      onInstalled();
    } else {
      error = formatError(res.error);
    }
  }

  // For the detail modal: a version whose file is externally hosted must open
  // its page (or the project page if the file URL is empty), never download.
  function externalOf(card: ModSummary, v: ModVersion): string | null {
    if (v.primary_file.distribution_allowed) return null;
    return v.primary_file.url.length > 0
      ? v.primary_file.url
      : modProjectUrl(card.source, card.slug ?? card.project_id, card.author);
  }

  function openUrl(url: string): void {
    void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(url));
  }
</script>

<div class="flex flex-col gap-2" data-testid="server-plugin-browser">
  <!-- Toolbar: source + search + pinned facets + sort + layout -->
  <div class="flex items-center gap-2">
    {#if showSourcePicker}
      <SourcePicker
        value={source}
        onChange={(v) => (source = v)}
        options={['modrinth', 'hangar']}
      />
    {/if}
    <input
      type="search"
      class="filter-control flex-1 min-w-[8rem]"
      placeholder={$t('servers.plugins.searchPlaceholder')}
      aria-label={$t('servers.plugins.searchPlaceholder')}
      bind:value={query}
      data-testid="server-plugin-search"
    />
    <span class="text-xs text-muted whitespace-nowrap"
      >{$t('servers.addons.pinnedFacets', { core: displayCore(core), mcVersion })}</span
    >
    <Select
      class="filter-control filter-control-select"
      value={sort}
      options={sortOptions}
      onChange={(v) => (sort = v as ModSort)}
      ariaLabel={$t('browse.filter.sortLabel')}
      dataTestid="server-plugin-sort"
    />
    <label class="flex shrink-0 items-center gap-1.5 text-xs text-secondary whitespace-nowrap">
      <input
        type="checkbox"
        class="accent-accent"
        bind:checked={showInstalled}
        data-testid="server-plugin-show-installed"
      />
      {$t('browse.filter.showInstalled')}
    </label>
    <LayoutToggle />
  </div>

  {#if error}
    <p class="text-sm text-danger">{error}</p>
  {:else if loading && hits.length === 0}
    <LoadingPanel label={$t('servers.mods.searching')} delayMs={0} />
  {:else if hits.length === 0}
    <p class="py-6 text-center text-sm text-muted">{$t('servers.plugins.noResults')}</p>
  {:else}
    <ModResultsGrid
      hits={visibleHits}
      layout={browserPrefs.layout}
      isMod={true}
      placeholderIcon="puzzle"
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
    loadVersions={() => commands.modsPluginVersions(d.source, d.project_id, mcVersion, core)}
    installVersion={(v) =>
      commands.serverInstallPlugin(serverId, d.source, d.project_id, v.version_id)}
    externalOf={(v) => externalOf(d, v)}
    openExternal={openUrl}
    projectUrl={modProjectUrl(d.source, d.slug ?? d.project_id, d.author)}
    onInstalled={(report) => {
      // Toast copy keys on the PROJECT name; the version label the modal
      // passes is not surfaced in the toast.
      toastInstalled(d.name, report);
      // Reflect the install in the browse cards behind the modal.
      void loadInstalled();
    }}
  />
{/if}
