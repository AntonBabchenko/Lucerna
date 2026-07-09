<script lang="ts">
  import { commands, type ModSource, type ModSummary, type ServerCore } from '$lib/ipc/bindings';
  import { SvelteSet } from 'svelte/reactivity';
  import { get } from 'svelte/store';
  import { t } from '$lib/i18n';
  import { formatError } from '$lib/ipc/format-error';
  import { modProjectUrl } from '$lib/mods/project-url';
  import { browserPrefs } from '$lib/mods/browser-prefs.svelte';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import ModResultsGrid from '$lib/mods/ModResultsGrid.svelte';
  import SourcePicker from '$lib/mods/SourcePicker.svelte';
  import Pagination from '$lib/ui/Pagination.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';

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
  }: {
    serverId: string;
    mcVersion: string;
    core: ServerCore;
    onInstalled: () => void;
  } = $props();

  let source = $state<ModSource>('modrinth');
  let query = $state('');
  let page = $state(0);
  let hits = $state<ModSummary[]>([]);
  let total = $state(0);
  let loading = $state(false);
  let error = $state<string | null>(null);
  // Projects whose install is in flight, keyed by project_id (a Set so two
  // installs can overlap without clobbering each other's busy state).
  const installing = new SvelteSet<string>();

  const pageSize = $derived(browserPrefs.pageSize);
  const pageCount = $derived(Math.max(1, Math.ceil(total / pageSize)));

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
      sort: 'downloads',
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
  // debounce delay on open); subsequent query/source changes debounce.
  // Driving everything from one $effect avoids the onMount + effect double-fire
  // that would issue two searches on every mount.
  let debounce: ReturnType<typeof setTimeout> | null = null;
  let firstRun = true;
  $effect(() => {
    const _ = query;
    const __ = source;
    if (debounce) clearTimeout(debounce);
    if (firstRun) {
      firstRun = false;
      void reload();
      return;
    }
    debounce = setTimeout(() => {
      void (async () => {
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

  function openExternalPage(url: string, card: ModSummary): void {
    const target =
      url.length > 0 ? url : modProjectUrl(card.source, card.slug ?? card.project_id, card.author);
    void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(target));
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
        const depCount = Math.max(0, res.data.installed.length - 1);
        const msg =
          depCount > 0
            ? get(t)('servers.plugins.installedWithDeps', { name: card.name, count: depCount })
            : get(t)('servers.plugins.installedOne', { name: card.name });
        pushSuccess(msg);
        if (res.data.unresolved.length > 0) {
          pushWarning(
            get(t)('servers.plugins.someDepsUnresolved', { deps: res.data.unresolved.join(', ') }),
          );
        }
        onInstalled();
      } else {
        error = formatError(res.error);
      }
    } finally {
      installing.delete(card.project_id);
    }
  }

  function openProject(card: ModSummary): void {
    const url = modProjectUrl(card.source, card.slug ?? card.project_id, card.author);
    void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(url));
  }
</script>

<div class="flex flex-col gap-2" data-testid="server-plugin-browser">
  <!-- Toolbar: source + search -->
  <div class="flex items-center gap-2">
    <SourcePicker value={source} onChange={(v) => (source = v)} options={['modrinth', 'hangar']} />
    <input
      type="search"
      class="filter-control flex-1 min-w-[8rem]"
      placeholder={$t('servers.plugins.searchPlaceholder')}
      aria-label={$t('servers.plugins.searchPlaceholder')}
      bind:value={query}
      data-testid="server-plugin-search"
    />
  </div>

  {#if error}
    <p class="text-sm text-danger">{error}</p>
  {:else if loading && hits.length === 0}
    <LoadingPanel label={$t('servers.mods.searching')} />
  {:else if hits.length === 0}
    <p class="py-6 text-center text-sm text-muted">{$t('servers.plugins.noResults')}</p>
  {:else}
    <ModResultsGrid
      {hits}
      layout="list"
      isMod={true}
      placeholderIcon="puzzle"
      installedFor={() => null}
      isCardBusy={(id) => installing.has(id)}
      onInstall={(h) => void install(h)}
      onOpenDetail={(h) => openProject(h)}
      onToggle={() => {}}
      onUninstall={() => {}}
    />
    <Pagination {page} {pageCount} disabled={loading} {onPage} />
  {/if}
</div>
