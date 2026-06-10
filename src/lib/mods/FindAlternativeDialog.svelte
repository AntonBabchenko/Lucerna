<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { commands } from '$lib/ipc/bindings';
  import type { LoaderKind, ModSource, ModSummary } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import { deriveSearchQuery, isPlausibleAlternative } from './alternative-match';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { Icon } from '$lib/ui/icons';

  // Reusable "find this mod on Modrinth" dialog for a mod the user cannot
  // auto-download (a modpack author disabled CurseForge distribution, or a
  // direct CF install hit the same wall). Auto-searches Modrinth by name,
  // scoped to the instance's MC + loader, and installs the chosen candidate
  // through the normal dependency-aware pipeline. `onInstalled` lets the
  // caller close a modpack `missing_mods` entry; the mod-browser caller omits
  // it (a plain install, nothing to link). Identity is fuzzy (no hash), so we
  // hint a likely match but never auto-install.
  let {
    modName,
    mcVersion,
    loader,
    instanceId,
    curseForgeUrl = null,
    onClose,
    onInstalled,
  }: {
    modName: string;
    mcVersion: string;
    loader: LoaderKind;
    instanceId: string;
    curseForgeUrl?: string | null;
    onClose: () => void;
    onInstalled?: (chosen: { source: ModSource; projectId: string }) => void | Promise<void>;
  } = $props();

  // Seed the editable query from the initial mod name; later edits are local.
  // svelte-ignore state_referenced_locally
  let query = $state(deriveSearchQuery(modName));
  let candidates = $state<ModSummary[] | null>(null);
  let searchError = $state<string | null>(null);
  let installError = $state<string | null>(null);
  let busyId = $state<string | null>(null);

  async function search() {
    candidates = null;
    searchError = null;
    installError = null;
    const usedQuery = query.trim();
    const r = await commands.modsSearch({
      source: 'modrinth',
      kind: 'mod',
      query: usedQuery,
      mc_version: mcVersion,
      loader,
      sort: 'relevance',
      page_size: 20,
      offset: 0,
    });
    if (r.status === 'error') {
      searchError = formatError(r.error);
      return;
    }
    // Show ONLY hits that plausibly ARE the searched mod (by name or slug).
    // Modrinth relevance search otherwise returns unrelated "close" hits that
    // must never be offered as a one-click alternative.
    candidates = r.data.hits.filter((h) => isPlausibleAlternative(usedQuery, h.name, h.slug));
  }

  async function install(card: ModSummary) {
    installError = null;
    busyId = card.project_id;
    try {
      const versions = await commands.modsVersions('modrinth', card.project_id, mcVersion, loader);
      if (versions.status === 'error') {
        installError = formatError(versions.error);
        return;
      }
      if (versions.data.length === 0) {
        installError = get(t)('mods.findAlt.noVersion', { mod: card.name });
        return;
      }
      const v = versions.data[0]!;
      const res = await commands.modsInstallWithDeps(
        instanceId,
        { source: 'modrinth', project_id: card.project_id, version_id: v.version_id },
        [],
      );
      if (res.status === 'error') {
        installError = formatError(res.error);
        return;
      }
      await onInstalled?.({ source: 'modrinth', projectId: card.project_id });
      pushSuccess(get(t)('mods.findAlt.installed', { name: card.name }), []);
      onClose();
    } finally {
      busyId = null;
    }
  }

  function openCurseForge() {
    const url = curseForgeUrl;
    if (url) {
      void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(url));
    }
  }

  onMount(() => {
    void search();
  });
</script>

<Modal
  ariaLabelledby="find-alt-title"
  onClose={onClose}
  panelClass="w-[min(40rem,92vw)] max-h-[85vh] flex flex-col"
>
    <header
      class="p-4 border-b flex items-center justify-between"
      data-testid="find-alt-dialog"
    >
      <h3 id="find-alt-title" class="font-medium text-primary">
        {$t('mods.findAlt.title', { mod: modName })}
      </h3>
      <CloseButton onClick={onClose} ariaLabel={$t('mods.findAlt.close')} />
    </header>

    <div class="p-4 overflow-y-auto flex-1">
      <p class="text-xs text-muted mb-3">{$t('mods.findAlt.body')}</p>

      <form
        class="flex gap-2 mb-3"
        onsubmit={(e) => {
          e.preventDefault();
          void search();
        }}
      >
        <input
          type="text"
          bind:value={query}
          class="filter-control flex-1"
          data-testid="find-alt-query"
          aria-label={$t('mods.findAlt.searchLabel')}
        />
        <button type="submit" class="btn-secondary btn-sm">{$t('mods.findAlt.searchBtn')}</button>
      </form>

      {#if installError}
        <p class="text-sm text-danger mb-2" data-testid="find-alt-install-error">{installError}</p>
      {/if}

      {#if candidates === null && !searchError}
        <p class="text-sm text-muted" data-testid="find-alt-loading">
          {$t('mods.findAlt.searching')}
        </p>
      {:else if searchError}
        <p class="text-sm text-danger" data-testid="find-alt-search-error">{searchError}</p>
      {:else if candidates && candidates.length === 0}
        <p class="text-sm text-muted" data-testid="find-alt-empty">{$t('mods.findAlt.empty')}</p>
      {:else if candidates}
        <ul class="space-y-2" data-testid="find-alt-results">
          {#each candidates as c (c.project_id)}
            <li class="flex items-center gap-3 p-2 rounded border border-subtle">
              {#if c.icon_url}
                <img src={c.icon_url} alt="" class="w-10 h-10 rounded flex-shrink-0" />
              {/if}
              <div class="min-w-0 flex-1">
                <span class="text-sm text-primary truncate">{c.name}</span>
                <span class="text-xs text-muted truncate block">{c.author}</span>
              </div>
              <BusyButton
                busy={busyId === c.project_id}
                disabled={busyId !== null && busyId !== c.project_id}
                class="btn-primary btn-xs flex-shrink-0"
                onclick={() => void install(c)}
              >
                {$t('mods.findAlt.install')}
              </BusyButton>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    {#if curseForgeUrl}
      <footer class="p-3 border-t text-center">
        <button
          type="button"
          class="text-accent hover:underline text-xs inline-flex items-center gap-1"
          onclick={openCurseForge}
        >
          {$t('mods.findAlt.openCurseForge')}
          <Icon name="externalLink" size={12} />
        </button>
      </footer>
    {/if}
</Modal>
