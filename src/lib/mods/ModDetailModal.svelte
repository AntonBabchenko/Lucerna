<script lang="ts">
  import {
    commands,
    type GalleryImage,
    type LoaderKind,
    type ModProject,
    type ModSource,
    type ModVersion,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import TabBar from '$lib/ui/TabBar.svelte';
  import ImageGallery from '$lib/ui/ImageGallery.svelte';
  import RenderedBody from '$lib/ui/RenderedBody.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';

  // Centered detail modal for a mod. Two tabs: Overview (gallery +
  // description + install-recommended) and Versions (full list with the
  // compatibility-off toggle). The owning ModBrowseView mounts/unmounts
  // this to open/close. Every command returns the tauri-specta
  // { status, data | error } shape — we branch explicitly.

  let {
    source,
    projectId,
    mcVersion,
    loader,
    installedVersionId = null,
    onClose,
    onInstall,
  }: {
    source: ModSource;
    projectId: string;
    mcVersion: string | null;
    loader: LoaderKind | null;
    installedVersionId?: string | null;
    onClose: () => void;
    onInstall: (v: ModVersion) => void;
  } = $props();

  type TabId = 'overview' | 'versions';
  let tab = $state<TabId>('overview');

  let project = $state<ModProject | null>(null);
  // Latest compatible versions (mc+loader). Drives the recommended button
  // AND the Versions list when show-all is off — so toggling show-all on
  // the Versions tab never changes what "recommended" means.
  let compatibleVersions = $state<ModVersion[] | null>(null);
  // Full (unfiltered) list, fetched lazily only when show-all is enabled.
  let allVersions = $state<ModVersion[] | null>(null);
  let showAll = $state(false);
  let error = $state<string | null>(null);

  const gallery = $derived<GalleryImage[]>(project?.gallery ?? []);
  const recommended = $derived(compatibleVersions?.[0] ?? null);
  const versionList = $derived(showAll ? allVersions : compatibleVersions);
  // Mod install needs a real (non-vanilla) loader + mc + an instance.
  const canInstall = $derived(mcVersion !== null && loader !== null && loader !== 'vanilla');

  const externalUrl = $derived(
    source === 'modrinth'
      ? `https://modrinth.com/mod/${project?.summary.slug ?? projectId}`
      : `https://www.curseforge.com/minecraft/mc-mods/${project?.summary.slug ?? projectId}`,
  );

  $effect(() => {
    void projectId;
    void load();
  });

  async function load() {
    error = null;
    const p = await commands.modsProject(source, projectId);
    if (p.status === 'ok') {
      project = p.data;
    } else {
      error = formatError(p.error);
      return;
    }
    if (mcVersion && loader && loader !== 'vanilla') {
      const v = await commands.modsVersions(source, projectId, mcVersion, loader);
      compatibleVersions = v.status === 'ok' ? v.data : [];
      if (v.status === 'error') error = formatError(v.error);
    } else {
      compatibleVersions = [];
    }
  }

  // Lazy-load the unfiltered list the first time show-all flips on.
  $effect(() => {
    if (showAll && allVersions === null) {
      void (async () => {
        const v = await commands.modsVersions(source, projectId, null, null);
        if (v.status === 'ok') allVersions = v.data;
        else error = formatError(v.error);
      })();
    }
  });

  function openExternal(url: string) {
    if (!url) return;
    void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(url));
  }
</script>

<div class="fixed inset-0 z-30 flex items-center justify-center">
  <button type="button" class="absolute inset-0 bg-black/30" aria-label="Close" onclick={onClose}
  ></button>
  <div
    role="dialog"
    aria-modal="true"
    class="relative bg-surface rounded shadow-lg w-full max-w-2xl lg:max-w-3xl xl:max-w-4xl 2xl:max-w-5xl max-h-[90vh] flex flex-col m-4"
  >
    <!-- Fixed header: title, source link, tabs stay put while the body scrolls. -->
    <div class="p-4 pb-0 shrink-0">
      <div class="flex items-start justify-between">
        <h2 class="text-base font-semibold text-primary flex-1">
          {project?.summary.name ?? 'Loading…'}
        </h2>
        <CloseButton onClick={onClose} ariaLabel="Close mod details" />
      </div>
      {#if project}
        <div class="text-xs text-muted mt-1">
          by {project.summary.author} · {project.summary.source} · {(
            project.summary.downloads ?? 0
          ).toLocaleString()} downloads
        </div>
        <button
          type="button"
          class="btn-tertiary text-xs mt-0.5"
          onclick={() => openExternal(externalUrl)}
        >
          View on {source === 'modrinth' ? 'Modrinth' : 'CurseForge'} ↗
        </button>
      {/if}

      <div class="mt-3">
        <TabBar
          tabs={[
            { id: 'overview', label: 'Overview' },
            { id: 'versions', label: 'Versions' },
          ]}
          active={tab}
          onChange={(id) => (tab = id as TabId)}
        />
      </div>
    </div>

    <!-- Scrollable body. -->
    <div class="flex-1 overflow-y-auto min-h-0 p-4 pt-3">
      {#if error}
        <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2 mb-3">
          {error}
        </div>
      {/if}

      {#if tab === 'overview'}
        <div class="space-y-3">
          {#if project && project.body_html}
            <p class="text-xs text-placeholder italic">
              Content from {source === 'modrinth' ? 'Modrinth' : 'CurseForge'} — any ads or links in it
              are the author's, not Lucerna's.
            </p>
          {/if}
          <ImageGallery images={gallery} />
          {#if project && project.body_html}
            <RenderedBody html={project.body_html} />
          {:else if project}
            <p class="text-sm text-secondary whitespace-pre-line selectable">
              {project.summary.summary}
            </p>
          {:else}
            <div class="flex justify-center py-8 text-secondary">
              <Spinner size="lg" label="Loading description…" />
            </div>
          {/if}
        </div>
      {:else}
        <div>
          <div class="flex items-center justify-end mb-2">
            <label class="inline-flex items-center gap-1 text-xs text-secondary">
              <input type="checkbox" bind:checked={showAll} data-testid="mod-detail-show-all" />
              Show all versions
            </label>
          </div>
          {#if versionList === null}
            <div class="flex justify-center py-8 text-secondary">
              <Spinner label="Loading versions…" />
            </div>
          {:else if versionList.length === 0}
            <div class="text-sm text-placeholder">
              {#if showAll}
                No versions returned by the platform.
              {:else}
                No compatible versions for this MC + loader.
              {/if}
            </div>
          {:else}
            {#each versionList as v (v.version_id)}
              {@const isInstalled = v.version_id === installedVersionId}
              {@const hasOtherInstalled =
                installedVersionId !== null && installedVersionId !== v.version_id}
              <div
                class="border-t py-2 flex items-center gap-2 text-sm {isInstalled
                  ? 'bg-success-bg'
                  : ''}"
              >
                <div class="flex-1 min-w-0">
                  <div class="truncate font-medium">
                    {v.version_number}{isInstalled ? ' · installed' : ''}
                  </div>
                  <div class="text-xs text-muted truncate">MC: {v.mc_versions.join(', ')}</div>
                </div>
                <button
                  type="button"
                  class="btn-xs {isInstalled
                    ? 'btn-secondary border-success text-success'
                    : !v.primary_file.distribution_allowed
                      ? 'btn-secondary text-muted'
                      : 'btn-primary'}"
                  disabled={!v.primary_file.distribution_allowed || isInstalled}
                  onclick={() => onInstall(v)}
                  title={hasOtherInstalled
                    ? `Switch from v${installedVersionId} to this version`
                    : undefined}
                >
                  {#if isInstalled}
                    ✓ Installed
                  {:else if !v.primary_file.distribution_allowed}
                    Restricted
                  {:else if hasOtherInstalled}
                    Switch
                  {:else}
                    Install
                  {/if}
                </button>
              </div>
            {/each}
          {/if}
        </div>
      {/if}
    </div>

    <!-- Sticky footer: the recommended-install CTA stays reachable without
         scrolling to the bottom of a long description. Overview only — the
         Versions tab installs per-row. -->
    {#if tab === 'overview' && compatibleVersions !== null}
      <div class="shrink-0 border-t border-border-subtle p-4 py-3">
        {#if canInstall && recommended}
          {@const isInstalled = recommended.version_id === installedVersionId}
          <button
            type="button"
            class="btn-primary w-full"
            disabled={isInstalled || !recommended.primary_file.distribution_allowed}
            onclick={() => onInstall(recommended)}
          >
            {#if isInstalled}
              ✓ Installed {recommended.version_number}
            {:else if !recommended.primary_file.distribution_allowed}
              Restricted — open on platform
            {:else}
              Install {recommended.version_number}
            {/if}
          </button>
        {:else}
          <div class="text-xs text-placeholder text-center">
            {#if !canInstall}
              Select a Fabric / Quilt / Forge / NeoForge instance to install mods.
            {:else}
              No compatible version for {mcVersion} / {loader}.
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>
