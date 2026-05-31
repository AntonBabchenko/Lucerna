<script lang="ts">
  import { commands } from '$lib/ipc/bindings';
  import type {
    GalleryImage,
    ModpackHit,
    ModpackProject,
    ModpackVersionEntry,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import TabBar from '$lib/ui/TabBar.svelte';
  import ImageGallery from '$lib/ui/ImageGallery.svelte';
  import RenderedBody from '$lib/ui/RenderedBody.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';

  // Centered detail modal for a modpack. Two tabs: Overview (gallery +
  // description + install-recommended) and Versions (full list + the
  // distribution-disabled fallback). Installing a pack creates a NEW
  // instance, so the recommended version is the newest visible one.

  let {
    hit,
    mcFilter = null,
    onClose,
    onInstall,
  }: {
    hit: ModpackHit;
    // MC version filter forwarded from the browse toolbar. When set, hide
    // pack versions whose `game_versions` don't include it. Null = show
    // every version.
    mcFilter?: string | null;
    onClose: () => void;
    onInstall: (tempPath: string, versionId: string) => void;
  } = $props();

  type TabId = 'overview' | 'versions';
  let tab = $state<TabId>('overview');

  let project = $state<ModpackProject | null>(null);
  let versions = $state<ModpackVersionEntry[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let downloading = $state(false);
  let installBlocked = $state(false);

  const blocked = $derived(hit.distribution_allowed === false || installBlocked);
  const visibleVersions = $derived(
    mcFilter ? versions.filter((v) => v.game_versions.includes(mcFilter)) : versions,
  );
  // Backend returns versions newest-first, so [0] is the recommended pick.
  const recommended = $derived(visibleVersions[0] ?? null);
  const gallery = $derived<GalleryImage[]>(project?.gallery ?? []);

  const platformName = $derived(hit.source === 'modrinth' ? 'Modrinth' : 'CurseForge');
  // Canonical project page on the source platform — the "View on …" link
  // under the title and the distribution-blocked "Open on CurseForge"
  // fallback both point here.
  const sourceUrl = $derived(
    hit.source === 'modrinth'
      ? `https://modrinth.com/modpack/${hit.slug}`
      : `https://www.curseforge.com/minecraft/modpacks/${hit.slug}`,
  );

  function openExternal(url: string) {
    void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(url));
  }

  $effect(() => {
    void hit.project_id;
    (async () => {
      loading = true;
      error = null;
      const [v, p] = await Promise.all([
        commands.modpackGetVersions(hit.source, hit.project_id),
        commands.modpackProject(hit.source, hit.project_id),
      ]);
      if (v.status === 'ok') versions = v.data;
      else error = formatError(v.error);
      // A project fetch failure is non-fatal — Overview just shows no body.
      if (p.status === 'ok') project = p.data;
      loading = false;
    })();
  });

  async function install(versionId: string) {
    downloading = true;
    try {
      const result = await commands.modpackFetchToTemp(hit.source, hit.project_id, versionId);
      if (result.status === 'ok') {
        onInstall(result.data, versionId);
      } else if (result.error.kind === 'modpack_cf_distribution_disabled') {
        installBlocked = true;
      } else {
        error = formatError(result.error);
      }
    } finally {
      downloading = false;
    }
  }
</script>

<div class="fixed inset-0 z-30 flex items-center justify-center">
  <button type="button" class="absolute inset-0 bg-black/30" aria-label="Close" onclick={onClose}
  ></button>
  <div
    class="relative bg-surface rounded shadow-lg w-full max-w-2xl lg:max-w-3xl xl:max-w-4xl 2xl:max-w-5xl max-h-[90vh] flex flex-col m-4"
    role="dialog"
    aria-modal="true"
    aria-label="Modpack details"
  >
    <header class="p-4 border-b flex items-start shrink-0">
      <div class="flex-1 min-w-0">
        <h3 class="font-semibold text-primary">{hit.title}</h3>
        <button
          type="button"
          class="btn-tertiary text-xs mt-0.5"
          onclick={() => openExternal(sourceUrl)}
        >
          View on {platformName} ↗
        </button>
      </div>
      <CloseButton onClick={onClose} ariaLabel="Close modpack details" />
    </header>

    {#if blocked}
      <div class="p-4 text-sm text-secondary">
        <p class="mb-3">
          The author of this CurseForge modpack disabled third-party launcher downloads, so it
          cannot be installed automatically. Open it on CurseForge to download the
          <code>.zip</code>, then import it with the drag-and-drop box above.
        </p>
        <button type="button" class="btn-secondary btn-sm" onclick={() => openExternal(sourceUrl)}>
          Open on CurseForge ↗
        </button>
      </div>
    {:else}
      <div class="px-4 pt-3 shrink-0">
        <TabBar
          tabs={[
            { id: 'overview', label: 'Overview' },
            { id: 'versions', label: 'Versions' },
          ]}
          active={tab}
          onChange={(id) => (tab = id as TabId)}
        />
      </div>

      <div class="flex-1 overflow-y-auto min-h-0 p-4">
        {#if error}
          <div class="text-sm text-danger mb-3">{error}</div>
        {/if}

        {#if tab === 'overview'}
          <div class="space-y-3">
            {#if project && project.body_html}
              <p class="text-xs text-placeholder italic">
                Content from {hit.source === 'modrinth' ? 'Modrinth' : 'CurseForge'} — any ads or links
                in it are the author's, not Lucerna's.
              </p>
            {/if}
            <ImageGallery images={gallery} />
            {#if project && project.body_html}
              <RenderedBody html={project.body_html} />
            {:else}
              <p class="text-sm text-secondary">{hit.description}</p>
            {/if}
          </div>
        {:else if loading}
          <div class="flex justify-center py-8 text-secondary">
            <Spinner label="Loading versions…" />
          </div>
        {:else if visibleVersions.length === 0}
          <div class="text-sm text-muted">
            {#if mcFilter}
              No versions for MC {mcFilter}. Clear the MC filter in the search to see all.
            {:else}
              No versions available.
            {/if}
          </div>
        {:else}
          <ul class="space-y-2">
            {#each visibleVersions as v (v.id)}
              <li class="p-2 border rounded text-sm">
                <div class="flex items-center">
                  <div class="flex-1 min-w-0">
                    <div class="font-medium truncate">{v.name}</div>
                    <div class="text-xs text-muted">
                      MC {v.game_versions.join(', ')} · {v.loaders.join(', ')}
                    </div>
                  </div>
                  <button
                    type="button"
                    class="btn-primary btn-xs ml-2"
                    disabled={downloading}
                    onclick={() => install(v.id)}
                  >
                    Install
                  </button>
                </div>
              </li>
            {/each}
          </ul>
        {/if}
      </div>

      <!-- Sticky footer: the install CTA stays reachable without scrolling
           past the gallery + description. Overview only — Versions installs
           per-row. -->
      {#if tab === 'overview' && !loading}
        <div class="shrink-0 border-t border-border-subtle p-4 py-3">
          {#if recommended}
            <button
              type="button"
              class="btn-primary w-full"
              disabled={downloading}
              onclick={() => install(recommended.id)}
            >
              Install {recommended.version_number}
            </button>
          {:else}
            <div class="text-xs text-placeholder text-center">
              {#if mcFilter}
                No versions for MC {mcFilter}. Clear the MC filter in the search to see all.
              {:else}
                No versions available.
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    {/if}
  </div>
</div>
