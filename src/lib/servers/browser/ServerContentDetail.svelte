<script lang="ts">
  // In-launcher detail card for the SERVER content browsers. Deliberately
  // content-kind-agnostic: it renders a project description + a version picker
  // and installs the CHOSEN version into a server, but it knows nothing about
  // whether the project is a mod or a plugin. Everything kind-specific — which
  // versions command to call, which install command to call, how to decide a
  // version is externally hosted, the project page URL, and the success/dep
  // toasts — is injected by the parent (ServerModBrowser / ServerPluginBrowser).
  //
  // This is NOT a retrofit of the instance ModDetailModal: that component is
  // instance-coupled (registry "installed" marks, dependency dialog, preflight,
  // asset install, compat/orphan flows). We copy its presentational shape only.
  //
  // Toasting lives in the PARENT: on a successful install we call
  // onInstalled(report, versionName) and let the parent reuse its existing
  // servers.mods.* / servers.plugins.* copy — the modal never duplicates it.
  import {
    commands,
    type GalleryImage,
    type InstallMissingReport,
    type ModProject,
    type ModSummary,
    type ModVersion,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import Modal from '$lib/ui/Modal.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import TabBar from '$lib/ui/TabBar.svelte';
  import ImageGallery from '$lib/ui/ImageGallery.svelte';
  import RenderedBody from '$lib/ui/RenderedBody.svelte';
  import { Icon } from '$lib/ui/icons';
  import { locale, t } from '$lib/i18n';
  import { formatCount } from '$lib/format/count';

  // The tauri-specta result envelopes, derived from the actual command
  // signatures so this component never hardcodes the { status, data|error }
  // shape. modsVersions and modsPluginVersions share the versions envelope;
  // serverInstallMod and serverInstallPlugin share the install envelope.
  // modsProject is source-dispatched and identical for mods and plugins (there
  // is no plugin-specific project command), so it is injected too — the parent
  // owns every IPC call and this component stays unit-testable via plain fns.
  type VersionsResult = Awaited<ReturnType<typeof commands.modsVersions>>;
  type InstallResult = Awaited<ReturnType<typeof commands.serverInstallMod>>;
  type ProjectResult = Awaited<ReturnType<typeof commands.modsProject>>;

  let {
    project,
    onClose,
    loadProject,
    loadVersions,
    installVersion,
    externalOf,
    openExternal,
    projectUrl,
    onInstalled,
  }: {
    project: ModSummary;
    onClose: () => void;
    /** Fetches the full project (body + gallery) for the Overview tab. Injected
     *  as `commands.modsProject(source, project_id)` by the parent. */
    loadProject: () => Promise<ProjectResult>;
    loadVersions: () => Promise<VersionsResult>;
    /** Receives the full chosen version, not just its id: the datapack
     *  browser's install command needs the whole `ModVersion`, and handing
     *  back the id alone forced callers to cache the fetched list by
     *  version_id to look it up again — a cache with no request-sequencing
     *  guard, so a slow-then-abandoned project's response could land after a
     *  fast one and overwrite the entry a subsequent install needed. This
     *  component already has the full object in scope at the call site
     *  (see `install` below), so it hands it back directly. */
    installVersion: (v: ModVersion) => Promise<InstallResult>;
    /** Per-version external file URL (e.g. Hangar externalUrl) when the file is
     *  hosted off-platform and must not be downloaded in-app; null otherwise. */
    externalOf: (v: ModVersion) => string | null;
    openExternal: (url: string) => void;
    /** Project page URL for the header "Open project page" link. */
    projectUrl: string;
    /** Called after a successful install; the parent toasts + refreshes. */
    onInstalled: (report: InstallMissingReport, versionName: string) => void;
  } = $props();

  type TabId = 'overview' | 'versions';
  // Default to Overview for parity with the client ModDetailModal: land on
  // "what is this project", switch to Versions to pick + install.
  let tab = $state<TabId>('overview');

  // Overview: null = still loading the full project; error surfaces separately.
  let projectDetail = $state<ModProject | null>(null);
  let projectError = $state<string | null>(null);

  // null = still loading; [] = loaded-but-empty.
  let versions = $state<ModVersion[] | null>(null);
  let error = $state<string | null>(null);
  // version_id whose install is in flight (at most one at a time — the whole
  // list is disabled while an install runs, and we close on success).
  let installingId = $state<string | null>(null);

  const installing = $derived(installingId !== null);
  const gallery = $derived<GalleryImage[]>(projectDetail?.gallery ?? []);
  // Human-facing platform name for the content disclaimer.
  const sourceLabel = $derived(
    project.source === 'modrinth'
      ? 'Modrinth'
      : project.source === 'curseforge'
        ? 'CurseForge'
        : 'Hangar',
  );

  // Re-fetch if the modal is reused for another project without unmounting
  // (mirrors ModDetailModal's `void projectId` guard).
  $effect(() => {
    void project.project_id;
    void loadProjectDetail();
    void load();
  });

  async function loadProjectDetail(): Promise<void> {
    projectError = null;
    projectDetail = null;
    const res = await loadProject();
    if (res.status === 'ok') {
      projectDetail = res.data;
    } else {
      projectError = formatError(res.error);
    }
  }

  async function load(): Promise<void> {
    error = null;
    versions = null;
    const res = await loadVersions();
    if (res.status === 'ok') {
      versions = res.data;
    } else {
      versions = [];
      error = formatError(res.error);
    }
  }

  function versionLabel(v: ModVersion): string {
    return v.version_number || v.name;
  }

  async function install(v: ModVersion): Promise<void> {
    installingId = v.version_id;
    error = null;
    try {
      const res = await installVersion(v);
      if (res.status === 'ok') {
        onInstalled(res.data, versionLabel(v));
        onClose();
      } else {
        error = formatError(res.error);
      }
    } finally {
      installingId = null;
    }
  }

  // formatCount, not toLocaleString(): the latter followed the OS locale, so a
  // Russian UI on an English Windows printed "12,345". $locale makes the app
  // language the source of truth, and re-renders on a live language switch.
  const downloads = $derived(formatCount($locale, project.downloads ?? 0));
</script>

<Modal
  ariaLabelledby="server-content-detail-title"
  {onClose}
  panelClass="w-full max-w-2xl lg:max-w-3xl max-h-[85vh] flex flex-col"
  closeOnBackdrop={!installing}
  closeOnEscape={!installing}
  dataTestid="server-content-detail"
>
  <!-- Fixed header: icon, title, author/downloads, project-page link. -->
  <div class="p-4 pb-3 shrink-0 border-b border-border-subtle">
    <div class="flex items-start gap-3">
      {#if project.icon_url}
        <img src={project.icon_url} alt="" class="h-10 w-10 shrink-0 rounded object-cover" />
      {:else}
        <div
          class="flex h-10 w-10 shrink-0 items-center justify-center rounded bg-subtle text-muted"
        >
          <Icon name="puzzle" size={20} />
        </div>
      {/if}
      <div class="min-w-0 flex-1">
        <h2 id="server-content-detail-title" class="text-base font-semibold text-primary truncate">
          {project.name}
        </h2>
        <div class="text-xs text-muted mt-0.5 truncate">
          {project.author} · {downloads}
        </div>
        <button
          type="button"
          class="btn-link text-xs mt-0.5 inline-flex items-center gap-1"
          onclick={() => openExternal(projectUrl)}
        >
          {$t('servers.contentDetail.openProjectPage')}
          <Icon name="externalLink" size={13} />
        </button>
      </div>
      <CloseButton onClick={onClose} ariaLabel={$t('common.close')} />
    </div>

    <div class="mt-3">
      <TabBar
        tabs={[
          { id: 'overview', label: $t('servers.contentDetail.tabOverview') },
          { id: 'versions', label: $t('servers.contentDetail.versions') },
        ]}
        active={tab}
        onChange={(id) => (tab = id as TabId)}
      />
    </div>
  </div>

  <!-- Scrollable body: Overview (description + gallery) or the version list. -->
  <div class="flex-1 overflow-y-auto min-h-0 p-4 pt-3">
    {#if tab === 'overview'}
      <div class="space-y-3">
        {#if projectError}
          <div
            class="bg-danger-bg border border-danger text-danger text-sm rounded p-2"
            data-testid="server-content-detail-overview-error"
          >
            {projectError}
          </div>
        {:else if projectDetail === null}
          <div class="flex justify-center py-8 text-secondary">
            <Spinner
              labelPlacement="below"
              label={$t('servers.contentDetail.loadingDescription')}
            />
          </div>
        {:else}
          {#if projectDetail.body_html}
            <p class="text-xs text-placeholder italic">
              {$t('servers.contentDetail.contentDisclaimer', { source: sourceLabel })}
            </p>
          {/if}
          <!-- ImageGallery renders nothing for an empty set — no empty box for
               Hangar (which has no gallery API). -->
          <ImageGallery images={gallery} />
          {#if projectDetail.body_html}
            <RenderedBody html={projectDetail.body_html} />
          {:else if project.summary}
            <p class="text-sm text-secondary whitespace-pre-line selectable">
              {project.summary}
            </p>
          {:else}
            <p class="py-6 text-center text-sm text-muted">
              {$t('servers.contentDetail.noDescription')}
            </p>
          {/if}
        {/if}
      </div>
    {:else}
      {#if error}
        <div
          class="bg-danger-bg border border-danger text-danger text-sm rounded p-2 mb-3"
          data-testid="server-content-detail-error"
        >
          {error}
        </div>
      {/if}

      {#if versions === null}
        <div class="flex justify-center py-8 text-secondary">
          <Spinner labelPlacement="below" label={$t('servers.contentDetail.loadingVersions')} />
        </div>
      {:else if versions.length === 0 && !error}
        <p class="py-6 text-center text-sm text-muted">
          {$t('servers.contentDetail.noVersions')}
        </p>
      {:else}
        {#each versions as v (v.version_id)}
          {@const external = externalOf(v)}
          <div
            class="border-t py-2 flex items-center gap-2 text-sm"
            data-testid="server-content-detail-version"
          >
            <div class="flex-1 min-w-0">
              <div class="truncate font-medium">{versionLabel(v)}</div>
              <div class="text-xs text-muted truncate">MC: {v.mc_versions.join(', ')}</div>
              {#if external}
                <div class="text-xs text-placeholder mt-0.5">
                  {$t('servers.contentDetail.externalNote')}
                </div>
              {/if}
            </div>
            {#if external}
              <button
                type="button"
                class="btn-secondary btn-sm shrink-0 inline-flex items-center gap-1"
                disabled={installing}
                onclick={() => openExternal(external)}
              >
                {$t('servers.contentDetail.openPage')}
                <Icon name="externalLink" size={13} />
              </button>
            {:else}
              <BusyButton
                class="btn-primary btn-sm shrink-0"
                busy={installingId === v.version_id}
                disabled={installing && installingId !== v.version_id}
                onclick={() => void install(v)}
              >
                {$t('common.install')}
              </BusyButton>
            {/if}
          </div>
        {/each}
      {/if}
    {/if}
  </div>
</Modal>
