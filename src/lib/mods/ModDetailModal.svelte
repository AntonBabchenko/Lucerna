<script lang="ts">
  import {
    commands,
    type ContentKind,
    type GalleryImage,
    type LoaderKind,
    type ModProject,
    type ModSource,
    type ModVersion,
    type RangeFamily,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { modProjectUrl } from '$lib/mods/project-url';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import TabBar from '$lib/ui/TabBar.svelte';
  import ImageGallery from '$lib/ui/ImageGallery.svelte';
  import { Icon } from '$lib/ui/icons';
  import RenderedBody from '$lib/ui/RenderedBody.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { t } from '$lib/i18n';
  import { tooltip } from '$lib/ui/tooltip';

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
    kind = 'mod',
    installedVersionId = null,
    installingVersionId = null,
    needed = null,
    family = null,
    onClose,
    onInstall,
  }: {
    source: ModSource;
    projectId: string;
    mcVersion: string | null;
    loader: LoaderKind | null;
    // Distinguishes mods (loader-aware, dependency-resolved install) from
    // loader-agnostic asset kinds (resource packs, shaders). Drives the
    // version-fetch guard and the install-eligibility check so that assets
    // never require a non-vanilla loader, and mods keep the original strict
    // loader requirement.
    kind?: ContentKind;
    installedVersionId?: string | null;
    // version_id whose install is currently in flight; parent-owned. The
    // matching version row / recommended CTA renders as a busy spinner.
    installingVersionId?: string | null;
    // When opened from the preflight picker: the dependency's declared range +
    // grammar. Drives a "fits range / out of range" badge per version. Both null
    // in normal mod-detail usage (no badge).
    needed?: string | null;
    family?: RangeFamily | null;
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
  // Mods need a real (non-vanilla) loader + mc version.
  // Assets (resource packs, shaders) only need mcVersion — they are
  // loader-agnostic so vanilla instances are valid install targets too.
  const canInstall = $derived(
    kind !== 'mod'
      ? mcVersion !== null
      : mcVersion !== null && loader !== null && loader !== 'vanilla',
  );

  const externalUrl = $derived(modProjectUrl(source, project?.summary.slug ?? projectId));

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
    if (kind !== 'mod') {
      // Asset kinds (resource packs, shaders) are loader-agnostic.
      // Fetch all MC-compatible versions when mcVersion is set; pass null
      // for loader so modsVersions returns every matching file regardless
      // of the instance's loader.
      if (mcVersion) {
        const v = await commands.modsVersions(source, projectId, mcVersion, null);
        compatibleVersions = v.status === 'ok' ? v.data : [];
        if (v.status === 'error') error = formatError(v.error);
      } else {
        compatibleVersions = [];
      }
    } else {
      // Mods: only load versions when a real (non-vanilla) loader + mc are set.
      if (mcVersion && loader && loader !== 'vanilla') {
        const v = await commands.modsVersions(source, projectId, mcVersion, loader);
        compatibleVersions = v.status === 'ok' ? v.data : [];
        if (v.status === 'error') error = formatError(v.error);
      } else {
        compatibleVersions = [];
      }
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

  // When opened from the preflight picker (needed + family set), badge each
  // listed version with whether it satisfies the dependency's declared range.
  // Pure backend filter — reuses the already-fetched list, no extra network.
  let satisfyingIds = $state<Set<string>>(new Set());
  $effect(() => {
    const list = versionList;
    if (!needed || !family || list === null || list.length === 0) {
      satisfyingIds = new Set();
      return;
    }
    // Guard against a stale resolve overwriting a newer one (e.g. fast show-all
    // toggles fire two concurrent filters): only the latest run may commit.
    let cancelled = false;
    void (async () => {
      const idx = await commands.modsFilterSatisfying(
        list.map((v) => v.version_number),
        needed,
        family,
      );
      if (!cancelled) satisfyingIds = new Set(idx.map((i) => list[i].version_id));
    })();
    return () => {
      cancelled = true;
    };
  });

  function openExternal(url: string) {
    if (!url) return;
    void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(url));
  }
</script>

<Modal
  ariaLabelledby="mod-detail-title"
  {onClose}
  panelClass="w-full max-w-2xl lg:max-w-3xl xl:max-w-4xl 2xl:max-w-5xl max-h-[90vh] flex flex-col"
>
  <!-- Fixed header: title, source link, tabs stay put while the body scrolls. -->
  <div class="p-4 pb-0 shrink-0">
    <div class="flex items-start justify-between">
      <h2 id="mod-detail-title" class="text-base font-semibold text-primary flex-1">
        {project?.summary.name ?? $t('mods.detail.loading')}
      </h2>
      <CloseButton onClick={onClose} ariaLabel={$t('mods.detail.closeAriaLabel')} />
    </div>
    {#if project}
      <div class="text-xs text-muted mt-1">
        {$t('mods.detail.byAuthorSourceDownloads', {
          author: project.summary.author,
          source: project.summary.source,
          downloads: (project.summary.downloads ?? 0).toLocaleString(),
        })}
      </div>
      <button
        type="button"
        class="btn-link text-xs mt-0.5 inline-flex items-center gap-1"
        onclick={() => openExternal(externalUrl)}
      >
        {source === 'modrinth'
          ? $t('mods.detail.viewOnModrinth')
          : $t('mods.detail.viewOnCurseForge')}
        <Icon name="externalLink" size={14} />
      </button>
    {/if}

    <div class="mt-3">
      <TabBar
        tabs={[
          { id: 'overview', label: $t('mods.detail.tabOverview') },
          { id: 'versions', label: $t('mods.detail.tabVersions') },
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
            {$t('mods.detail.contentDisclaimer', {
              source: source === 'modrinth' ? 'Modrinth' : 'CurseForge',
            })}
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
            <Spinner
              size="lg"
              labelPlacement="below"
              label={$t('mods.detail.loadingDescription')}
            />
          </div>
        {/if}
      </div>
    {:else}
      <div>
        <div class="flex items-center justify-end mb-2">
          <label class="inline-flex items-center gap-1 text-xs text-secondary">
            <input type="checkbox" bind:checked={showAll} data-testid="mod-detail-show-all" />
            {$t('mods.detail.showAllVersions')}
          </label>
        </div>
        {#if versionList === null}
          <div class="flex justify-center py-8 text-secondary">
            <Spinner labelPlacement="below" label={$t('mods.detail.loadingVersions')} />
          </div>
        {:else if versionList.length === 0}
          <div class="text-sm text-placeholder">
            {#if showAll}
              {$t('mods.detail.noVersionsPlatform')}
            {:else}
              {$t('mods.detail.noVersionsCompat')}
            {/if}
          </div>
        {:else}
          {#each versionList as v (v.version_id)}
            {@const isInstalled = v.version_id === installedVersionId}
            {@const hasOtherInstalled =
              installedVersionId !== null && installedVersionId !== v.version_id}
            {@const distAllowed = v.primary_file.distribution_allowed}
            {@const rowTooltip = isInstalled
              ? $t('common.installed')
              : !distAllowed
                ? $t('common.restricted')
                : hasOtherInstalled
                  ? $t('common.switchToVersion')
                  : $t('common.install')}
            <div
              class="border-t py-2 flex items-center gap-2 text-sm {isInstalled
                ? 'bg-success-bg'
                : ''}"
            >
              <div class="flex-1 min-w-0">
                <div class="truncate font-medium">
                  {v.version_number}{isInstalled ? $t('mods.detail.versionInstalled') : ''}
                </div>
                <div class="text-xs text-muted truncate">MC: {v.mc_versions.join(', ')}</div>
                {#if needed}
                  <span
                    class="mt-0.5 inline-block text-[10px] px-1 rounded {satisfyingIds.has(
                      v.version_id,
                    )
                      ? 'bg-success-bg text-success'
                      : 'bg-warning-text/10 text-warning-text'}"
                  >
                    {satisfyingIds.has(v.version_id)
                      ? $t('mods.preflight.pickerFits')
                      : $t('mods.preflight.pickerUnfit')}
                  </span>
                {/if}
              </div>
              <span class="inline-flex" use:tooltip={rowTooltip}>
                <BusyButton
                  busy={installingVersionId === v.version_id}
                  class={`btn-icon btn-icon-sm ${isInstalled
                    ? '!text-success'
                    : !distAllowed
                      ? '!text-muted'
                      : '!text-accent'}`}
                  disabled={!distAllowed || isInstalled}
                  aria-label={rowTooltip}
                  onclick={() => onInstall(v)}
                >
                  {#if isInstalled}
                    <Icon name="success" size={15} />
                  {:else if !distAllowed}
                    <Icon name="lock" size={15} />
                  {:else if hasOtherInstalled}
                    <Icon name="switch" size={15} />
                  {:else}
                    <Icon name="download" size={15} />
                  {/if}
                </BusyButton>
              </span>
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
        <BusyButton
          busy={installingVersionId === recommended.version_id}
          class="btn-primary w-full"
          disabled={isInstalled || !recommended.primary_file.distribution_allowed}
          onclick={() => onInstall(recommended)}
        >
          {#if isInstalled}
            <Icon name="success" size={14} />
            {$t('mods.detail.footerInstalled', { version: recommended.version_number })}
          {:else if !recommended.primary_file.distribution_allowed}
            {$t('mods.detail.footerRestricted')}
          {:else}
            <Icon name="download" size={14} />
            {$t('common.installVersion', { version: recommended.version_number })}
          {/if}
        </BusyButton>
      {:else}
        <div class="text-xs text-placeholder text-center">
          {#if !canInstall}
            {#if kind !== 'mod'}
              {$t('mods.browse.errorNoInstance')}
            {:else}
              {$t('mods.detail.selectInstanceHint')}
            {/if}
          {:else if kind !== 'mod'}
            {$t('mods.detail.noCompatVersionAsset', { mc: mcVersion ?? '' })}
          {:else}
            {$t('mods.detail.noCompatVersion', { mc: mcVersion ?? '', loader: loader ?? '' })}
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</Modal>
