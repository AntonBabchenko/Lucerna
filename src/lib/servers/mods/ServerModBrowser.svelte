<script lang="ts">
  import { commands, type LoaderKind, type ModSource, type ModSummary } from '$lib/ipc/bindings';
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
  import Pagination from '$lib/ui/Pagination.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';

  // A lighter, server-targeted mod browser (S2 #3). Deliberately NOT a retrofit
  // of the instance ModBrowseView — that component is deeply instance-coupled
  // (registry-aware "installed" marks, dependency dialog, preflight, asset
  // install, compat/orphan flows). This reuses only the target-agnostic low-level
  // pieces (modsSearch / modsVersions / ModResultsGrid / ModCard) and drives a
  // server-targeted install (serverInstallMod). Mods only — no RP/shaders, no
  // loader/MC facets (the server's are fixed). Auto-picks the newest compatible
  // version; dependency resolution happens in the backend kernel.

  let {
    serverId,
    mcVersion,
    loader,
    onInstalled,
  }: {
    serverId: string;
    mcVersion: string;
    loader: LoaderKind;
    onInstalled: () => void;
  } = $props();

  let source = $state<ModSource>('modrinth');
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

  const pageSize = $derived(browserPrefs.pageSize);
  const pageCount = $derived(Math.max(1, Math.ceil(total / pageSize)));

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
      kind: 'mod',
      query,
      mc_version: mcVersion,
      loader,
      sort: 'downloads',
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
  // debounce delay on open); subsequent query/source/cf-key changes debounce.
  // Driving everything from one $effect avoids the onMount + effect double-fire
  // that would issue two searches on every mount.
  let debounce: ReturnType<typeof setTimeout> | null = null;
  let firstRun = true;
  $effect(() => {
    const _ = query;
    const __ = source;
    const ___ = cfKeyVersion.value;
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

  async function install(card: ModSummary): Promise<void> {
    installing.add(card.project_id);
    error = null;
    try {
      const versions = await commands.modsVersions(card.source, card.project_id, mcVersion, loader);
      if (versions.status !== 'ok') {
        error = formatError(versions.error);
        return;
      }
      if (versions.data.length === 0) {
        pushWarning(get(t)('servers.mods.noCompatibleVersion', { name: card.name }));
        return;
      }
      // modsVersions returns newest-first (same assumption the dep resolver uses).
      const newest = versions.data[0];
      const res = await commands.serverInstallMod(
        serverId,
        card.source,
        card.project_id,
        newest.version_id,
      );
      if (res.status === 'ok') {
        const depCount = Math.max(0, res.data.installed.length - 1);
        const msg =
          depCount > 0
            ? get(t)('servers.mods.installedWithDeps', { name: card.name, count: depCount })
            : get(t)('servers.mods.installedOne', { name: card.name });
        pushSuccess(msg);
        if (res.data.unresolved.length > 0) {
          pushWarning(
            get(t)('servers.mods.someDepsUnresolved', { deps: res.data.unresolved.join(', ') }),
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
    const url = modProjectUrl(card.source, card.slug ?? card.project_id);
    void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(url));
  }
</script>

<div class="flex flex-col gap-2" data-testid="server-mod-browser">
  <!-- Toolbar: source + search -->
  <div class="flex items-center gap-2">
    <SourcePicker value={source} onChange={(v) => (source = v)} />
    <input
      type="search"
      class="filter-control flex-1 min-w-[8rem]"
      placeholder={$t('servers.mods.searchPlaceholder')}
      aria-label={$t('servers.mods.searchPlaceholder')}
      bind:value={query}
      data-testid="server-mod-search"
    />
  </div>

  {#if needsCfKey}
    <CurseForgeKeyBanner onOpenSettings={() => (settingsOpen.value = { tab: 'integrations' })} />
  {:else if error}
    <p class="text-sm text-danger">{error}</p>
  {:else if loading && hits.length === 0}
    <LoadingPanel label={$t('servers.mods.searching')} />
  {:else if hits.length === 0}
    <p class="py-6 text-center text-sm text-muted">{$t('servers.mods.noResults')}</p>
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
