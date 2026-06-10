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
  import { Icon } from '$lib/ui/icons';
  import RenderedBody from '$lib/ui/RenderedBody.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { t } from '$lib/i18n';

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

  const platformName = $derived(
    hit.source === 'modrinth'
      ? 'Modrinth'
      : hit.source === 'ftb'
        ? 'FTB'
        : hit.source === 'atlauncher'
          ? 'ATLauncher'
          : 'CurseForge',
  );
  // Canonical project page on the source platform — the "View on …" link
  // under the title and the distribution-blocked "Open on CurseForge"
  // fallback both point here. FTB pages are keyed by numeric pack id at
  // feed-the-beast.com/modpacks/<id> (the slug suffix is optional), and
  // hit.project_id IS that numeric id for FTB hits.
  const sourceUrl = $derived(
    hit.source === 'modrinth'
      ? `https://modrinth.com/modpack/${hit.slug}`
      : hit.source === 'curseforge'
        ? `https://www.curseforge.com/minecraft/modpacks/${hit.slug}`
        : hit.source === 'atlauncher'
          ? null
          : `https://www.feed-the-beast.com/modpacks/${hit.project_id}`,
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
    } catch (e) {
      // Never swallow a thrown/rejected invoke — surface it instead of
      // leaving the button dim with no feedback.
      error = e instanceof Error ? e.message : String(e);
    } finally {
      downloading = false;
    }
  }
</script>

<div class="fixed inset-0 z-30 flex items-center justify-center">
  <button
    type="button"
    class="absolute inset-0 bg-black/30"
    aria-label={$t('modpacks.detail.closeScrimAriaLabel')}
    onclick={onClose}
  ></button>
  <div
    class="relative bg-surface rounded shadow-lg w-full max-w-2xl lg:max-w-3xl xl:max-w-4xl 2xl:max-w-5xl max-h-[90vh] flex flex-col m-4"
    role="dialog"
    aria-modal="true"
    aria-label={$t('modpacks.detail.dialogAriaLabel')}
  >
    <header class="p-4 border-b flex items-start shrink-0">
      <div class="flex-1 min-w-0">
        <h3 class="font-semibold text-primary">{hit.title}</h3>
        {#if sourceUrl}
          <button
            type="button"
            class="btn-tertiary text-xs mt-0.5 inline-flex items-center gap-1"
            onclick={() => openExternal(sourceUrl)}
          >
            {$t('modpacks.detail.viewOn', { platform: platformName })}
            <Icon name="externalLink" size={14} />
          </button>
        {/if}
      </div>
      <CloseButton onClick={onClose} ariaLabel={$t('modpacks.detail.closeAriaLabel')} />
    </header>

    {#if blocked}
      <div class="p-4 text-sm text-secondary">
        <p class="mb-3">
          {$t('modpacks.detail.blockedBody')}
        </p>
        {#if sourceUrl}
          <button
            type="button"
            class="btn-secondary btn-sm inline-flex items-center gap-1"
            onclick={() => openExternal(sourceUrl)}
          >
            {$t('modpacks.detail.openOnCurseForge')}
            <Icon name="externalLink" size={14} />
          </button>
        {/if}
      </div>
    {:else}
      <div class="px-4 pt-3 shrink-0">
        <TabBar
          tabs={[
            { id: 'overview', label: $t('modpacks.detail.tabOverview') },
            { id: 'versions', label: $t('modpacks.detail.tabVersions') },
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
                {$t('modpacks.detail.contentDisclaimer', {
                  source: platformName,
                })}
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
            <Spinner label={$t('modpacks.detail.loadingVersions')} />
          </div>
        {:else if visibleVersions.length === 0}
          <div class="text-sm text-muted">
            {#if mcFilter}
              {$t('modpacks.detail.noVersionsForMc', { mc: mcFilter })}
            {:else}
              {$t('modpacks.detail.noVersions')}
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
                  <BusyButton
                    class="btn-primary btn-xs ml-2"
                    busy={downloading}
                    onclick={() => install(v.id)}
                  >
                    {$t('modpacks.detail.install')}
                  </BusyButton>
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
            <BusyButton
              class="btn-primary w-full"
              busy={downloading}
              onclick={() => install(recommended.id)}
            >
              {$t('modpacks.detail.installVersion', { version: recommended.version_number })}
            </BusyButton>
          {:else}
            <div class="text-xs text-placeholder text-center">
              {#if mcFilter}
                {$t('modpacks.detail.noVersionsForMc', { mc: mcFilter })}
              {:else}
                {$t('modpacks.detail.noVersions')}
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    {/if}
  </div>
</div>
