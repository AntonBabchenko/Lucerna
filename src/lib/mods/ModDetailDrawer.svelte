<script lang="ts">
  import {
    commands,
    type LoaderKind,
    type ModProject,
    type ModSource,
    type ModVersion,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';

  // Right-side drawer that loads a ModProject + the version list scoped
  // to the active instance's MC + loader pair. The owning view
  // (ModBrowseView) tracks which project is open through `drawerProject`
  // and unmounts this component to close.
  //
  // The plan's snippet uses try/catch, but every command on the bindings
  // surface returns the tauri-specta result-status shape:
  //   { status: 'ok', data } | { status: 'error', error }
  // so we branch explicitly and route the IPC Error through formatError
  // — same convention ModBrowseView uses.

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
    // Version id currently on disk for this project, if any. Highlights
    // the matching row and switches the Install label to "Reinstall" /
    // "Switch" so the user knows what clicking another version means.
    installedVersionId?: string | null;
    onClose: () => void;
    onInstall: (v: ModVersion) => void;
  } = $props();

  let project = $state<ModProject | null>(null);
  let versions = $state<ModVersion[] | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
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
    if (mcVersion && loader) {
      const v = await commands.modsVersions(source, projectId, mcVersion, loader);
      if (v.status === 'ok') {
        versions = v.data;
      } else {
        error = formatError(v.error);
      }
    } else {
      versions = [];
    }
  }

  function openExternal(url: string) {
    if (!url) return;
    void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(url));
  }

  // Canonical project page on the source platform. We always have a
  // slug-or-id we can use; this lets the user read the full markdown
  // description and screenshots in their browser, which the launcher
  // does not render itself (Modrinth's `body` field is markdown +
  // embedded HTML — rendering it inline would mean adopting a new
  // markdown parser, deferred post-v0.5.0).
  const externalUrl = $derived(
    source === 'modrinth'
      ? `https://modrinth.com/mod/${project?.summary.slug ?? projectId}`
      : `https://www.curseforge.com/minecraft/mc-mods/${project?.summary.slug ?? projectId}`,
  );
</script>

<div class="fixed inset-0 z-30 flex">
  <button type="button" class="flex-1 bg-black/30" aria-label="Close" onclick={onClose}></button>
  <div role="dialog" aria-modal="true" class="w-[420px] bg-white shadow-xl overflow-y-auto p-4">
    <div class="flex items-start justify-between">
      <h2 class="text-base font-semibold flex-1">
        {project?.summary.name ?? 'Loading…'}
      </h2>
      <button
        type="button"
        class="text-neutral-500 hover:text-neutral-700"
        aria-label="Close"
        onclick={onClose}>×</button
      >
    </div>
    {#if project}
      <div class="text-xs text-neutral-500 mt-1">
        by {project.summary.author} · {project.summary.source} · {(
          project.summary.downloads ?? 0
        ).toLocaleString()} downloads
      </div>
      <p class="text-sm text-neutral-700 mt-3 whitespace-pre-line">
        {project.summary.summary}
      </p>
      <div class="text-xs mt-2 flex gap-3 flex-wrap">
        <button
          type="button"
          class="text-blue-600 hover:underline"
          onclick={() => openExternal(externalUrl)}
        >
          Read full description on {source === 'modrinth' ? 'Modrinth' : 'CurseForge'} ↗
        </button>
        {#if project.website_url}
          <button
            type="button"
            class="text-blue-600 hover:underline"
            onclick={() => openExternal(project?.website_url ?? '')}
          >
            Project website ↗
          </button>
        {/if}
      </div>
    {/if}

    {#if error}
      <div class="bg-red-50 border border-red-200 text-red-900 text-sm rounded p-2 mt-3">
        {error}
      </div>
    {/if}

    {#if versions}
      <div class="mt-4">
        <div class="text-xs uppercase tracking-wide text-neutral-500 mb-2">
          Versions for {mcVersion ?? '?'} / {loader ?? '?'}
        </div>
        {#if versions.length === 0}
          <div class="text-sm text-neutral-400">No compatible versions for this MC + loader.</div>
        {:else}
          {#each versions as v (v.version_id)}
            {@const isInstalled = v.version_id === installedVersionId}
            {@const hasOtherInstalled =
              installedVersionId !== null && installedVersionId !== v.version_id}
            <div
              class="border-t py-2 flex items-center gap-2 text-sm"
              class:bg-green-50={isInstalled}
            >
              <div class="flex-1 min-w-0">
                <div class="truncate font-medium">
                  {v.version_number}{isInstalled ? ' · installed' : ''}
                </div>
                <div class="text-xs text-neutral-500 truncate">
                  MC: {v.mc_versions.join(', ')}
                </div>
              </div>
              <button
                type="button"
                class="px-2 py-0.5 text-xs rounded disabled:opacity-50"
                class:bg-blue-600={!isInstalled && v.primary_file.distribution_allowed}
                class:hover:bg-blue-700={!isInstalled && v.primary_file.distribution_allowed}
                class:text-white={!isInstalled && v.primary_file.distribution_allowed}
                class:border={isInstalled}
                class:border-green-300={isInstalled}
                class:text-green-700={isInstalled}
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
</div>
