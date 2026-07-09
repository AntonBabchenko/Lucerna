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
    type InstallMissingReport,
    type ModSummary,
    type ModVersion,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import Modal from '$lib/ui/Modal.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { Icon } from '$lib/ui/icons';
  import { t } from '$lib/i18n';

  // The tauri-specta result envelopes, derived from the actual command
  // signatures so this component never hardcodes the { status, data|error }
  // shape. modsVersions and modsPluginVersions share the versions envelope;
  // serverInstallMod and serverInstallPlugin share the install envelope.
  type VersionsResult = Awaited<ReturnType<typeof commands.modsVersions>>;
  type InstallResult = Awaited<ReturnType<typeof commands.serverInstallMod>>;

  let {
    project,
    onClose,
    loadVersions,
    installVersion,
    externalOf,
    openExternal,
    projectUrl,
    onInstalled,
  }: {
    project: ModSummary;
    onClose: () => void;
    loadVersions: () => Promise<VersionsResult>;
    installVersion: (versionId: string) => Promise<InstallResult>;
    /** Per-version external file URL (e.g. Hangar externalUrl) when the file is
     *  hosted off-platform and must not be downloaded in-app; null otherwise. */
    externalOf: (v: ModVersion) => string | null;
    openExternal: (url: string) => void;
    /** Project page URL for the header "Open project page" link. */
    projectUrl: string;
    /** Called after a successful install; the parent toasts + refreshes. */
    onInstalled: (report: InstallMissingReport, versionName: string) => void;
  } = $props();

  // null = still loading; [] = loaded-but-empty.
  let versions = $state<ModVersion[] | null>(null);
  let error = $state<string | null>(null);
  // version_id whose install is in flight (at most one at a time — the whole
  // list is disabled while an install runs, and we close on success).
  let installingId = $state<string | null>(null);

  const installing = $derived(installingId !== null);

  $effect(() => {
    void load();
  });

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
      const res = await installVersion(v.version_id);
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

  const downloads = $derived((project.downloads ?? 0).toLocaleString());
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

    {#if project.summary}
      <p class="text-sm text-secondary whitespace-pre-line selectable mt-3">
        {project.summary}
      </p>
    {/if}
  </div>

  <!-- Scrollable version list. -->
  <div class="flex-1 overflow-y-auto min-h-0 p-4 pt-3">
    <h3 class="text-xs font-semibold uppercase tracking-wide text-muted mb-2">
      {$t('servers.contentDetail.versions')}
    </h3>

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
  </div>
</Modal>
