<script lang="ts">
  import { onDestroy } from 'svelte';
  import {
    commands,
    type ModSummary,
    type ModUpdateState,
    type ModVersion,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { modProjectUrl } from '$lib/mods/project-url';
  import { pluginCapable } from '$lib/servers/core-display';
  import { serverState } from '$lib/servers/server-state.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import Select from '$lib/ui/Select.svelte';
  import ToggleChipGroup from '$lib/ui/ToggleChipGroup.svelte';
  import ServerContentDetail from '$lib/servers/browser/ServerContentDetail.svelte';
  import {
    createInstalledFilters,
    type SortBy,
    type ViewFilter,
  } from '$lib/mods/installed/installed-filters.svelte';
  import ServerInstalledRow from './ServerInstalledRow.svelte';
  import { createServerInstalledData, type ServerRow } from './server-installed-data.svelte';
  import {
    autoUpdateTargets,
    countAutoUpdatable,
    externalUpdateUrl,
    hasUpdate,
    isAutoUpdatable,
  } from './plugin-update-actions';

  let { serverId, reloadToken = 0 }: { serverId: string; reloadToken?: number } = $props();

  // Enriched Installed list. Plugins carry no quarantine reason (rows' `reason`
  // is always null → ServerInstalledRow shows no badge).
  const data = createServerInstalledData(
    () => serverId,
    'plugin',
    () => reloadToken,
  );

  // Search / enabled-disabled / sort over the installed list, hosted on the
  // shared client composable. No status predicates — plugins have no
  // updates / issues / incompatible views, so those chips stay at 0. Server
  // rows carry no install timestamp, so the sort is name-only.
  // Per-plugin update-check results, keyed by sha1 (identity that survives an
  // enable/disable rename).
  let updateChecks = $state(new Map<string, ModUpdateState>());
  let checkingUpdates = $state(false);
  // Per-row in-flight guard: `serverUpdatePluginOne` is a filesystem-mutating
  // swap with no backend concurrent-same-plugin guard. Keyed by sha1.
  let updatingShas = $state(new Set<string>());
  let updatingAll = $state(false);

  // A different server's checks must never bleed across a switch (sha1 keys can
  // collide across servers). Reset on serverId change.
  $effect(() => {
    void serverId;
    updateChecks = new Map();
  });

  const filters = createInstalledFilters(
    () => data.rows,
    (r) => ({
      id: r.sha1,
      name: r.card.summary?.name ?? r.card.installed.name,
      enabled: r.card.installed.enabled,
      sortKey: '',
      source: r.card.installed.source,
    }),
    { isUpdatable: (id) => hasUpdate(updateChecks.get(id)) },
  );
  onDestroy(() => {
    data.dispose();
    filters.dispose();
  });

  const sortOptions = $derived([
    { value: 'name-asc', label: $t('mods.installed.sortNameAsc') },
    { value: 'name-desc', label: $t('mods.installed.sortNameDesc') },
  ]);

  const filterOptions = $derived([
    {
      value: 'all',
      label: $t('mods.installed.filterAllLabel'),
      tone: 'neutral' as const,
      count: filters.counts.total,
    },
    {
      value: 'enabled',
      label: $t('mods.installed.filterEnabledLabel'),
      tone: 'success' as const,
      count: filters.counts.enabled,
    },
    {
      value: 'disabled',
      label: $t('mods.installed.filterDisabledLabel'),
      tone: 'muted' as const,
      count: filters.counts.disabled,
    },
    ...(filters.counts.updates > 0
      ? [
          {
            value: 'updates',
            label: $t('mods.installed.filterUpdatesLabel'),
            tone: 'warning' as const,
            icon: 'arrowUp' as const,
            count: filters.counts.updates,
          },
        ]
      : []),
  ]);

  let actionError = $state<string | null>(null);
  let busyFolder = $state(false);
  let pendingDelete = $state<ServerRow | null>(null);
  let deleting = $state(false);
  // The enriched project whose in-launcher detail card is open (null = closed).
  // Only identity-bearing rows (card.summary != null) can open it.
  let detail = $state<ModSummary | null>(null);

  // For the detail modal: a version whose file is externally hosted must open
  // its page (or the project page when the file URL is empty), never download
  // (mirrors ServerPluginBrowser's Hangar externalUrl handling).
  function externalOf(card: ModSummary, v: ModVersion): string | null {
    if (v.primary_file.distribution_allowed) return null;
    return v.primary_file.url.length > 0
      ? v.primary_file.url
      : modProjectUrl(card.source, card.slug ?? card.project_id, card.author);
  }

  // Dynamic import mirrors every other opener call-site in the app (the Tauri
  // plugin is never statically imported — it isn't resolvable under vitest/SSR
  // and the browsers use this exact helper).
  function openUrl(url: string): void {
    void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(url));
  }

  // Plugins only attach to plugin-capable cores (paper/purpur) — a mod-loader
  // or vanilla server gets no Plugins UI, just the requiresCore hint below.
  // Mutations require a stopped server (the backend enforces it; the UI gates
  // to avoid pointless errors).
  const server = $derived(serverState.list.find((s) => s.id === serverId) ?? null);
  const isPluginCore = $derived(server !== null && pluginCapable(server.loader));
  const isRunning = $derived(server?.running ?? false);
  const canManage = $derived(isPluginCore && !isRunning);

  // Non-external pending updates drive the "Update all" label + enablement.
  // External-hosted targets open a page individually and are excluded here.
  const updatableCount = $derived(countAutoUpdatable(data.rows, updateChecks));

  // Read-only scan: classify every identity-bearing plugin against its platform.
  async function checkUpdates() {
    checkingUpdates = true;
    actionError = null;
    try {
      const res = await commands.serverCheckPluginUpdates(serverId);
      if (res.status === 'ok') {
        const m = new Map<string, ModUpdateState>();
        for (const c of res.data) m.set(c.sha1, c.state);
        updateChecks = m;
      } else actionError = formatError(res.error);
    } finally {
      checkingUpdates = false;
    }
  }

  // Apply one pending update. Externally-hosted targets (Hangar) can't be
  // auto-downloaded — open the project/external page instead and leave the row
  // flagged. Otherwise swap the jar via the backend, then drop the stale check.
  async function updateOne(row: ServerRow) {
    const state = updateChecks.get(row.sha1);
    if (!hasUpdate(state)) return;
    if (!isAutoUpdatable(state)) {
      const url = externalUpdateUrl(state.target, row.card.summary);
      if (url) openUrl(url);
      else actionError = $t('servers.plugins.externalDownload', { name: row.card.installed.name });
      return;
    }
    // A stale badge can survive a server start — refuse the swap the backend
    // would reject anyway, with a clear message.
    if (!canManage) {
      actionError = $t('servers.plugins.stopToManage');
      return;
    }
    if (updatingShas.has(row.sha1)) return;
    updatingShas = new Set(updatingShas).add(row.sha1);
    actionError = null;
    try {
      const res = await commands.serverUpdatePluginOne(serverId, row.sha1, state.target);
      if (res.status === 'ok') {
        const next = new Map(updateChecks);
        next.delete(row.sha1);
        updateChecks = next;
        await data.refresh();
      } else actionError = formatError(res.error);
    } finally {
      const s = new Set(updatingShas);
      s.delete(row.sha1);
      updatingShas = s;
    }
  }

  // Apply every auto-updatable pending update in one sequential batch (each swap
  // mutates the plugins dir, so applies MUST stay serial). External-hosted rows
  // are skipped (they open a page individually) and remain flagged.
  async function updateAll() {
    if (!canManage) {
      actionError = $t('servers.plugins.stopToManage');
      return;
    }
    const targets = autoUpdateTargets(data.rows, updateChecks);
    if (targets.length === 0) return;
    actionError = null;
    updatingAll = true;
    try {
      for (const { sha, target } of targets) {
        const res = await commands.serverUpdatePluginOne(serverId, sha, target);
        if (res.status === 'error') {
          actionError = formatError(res.error);
          break;
        }
      }
      // Drop only the applied (auto-updatable) checks; keep external ones flagged.
      const next = new Map(updateChecks);
      for (const { sha } of targets) next.delete(sha);
      updateChecks = next;
      await data.refresh();
    } finally {
      updatingAll = false;
    }
  }

  // Toggle enable/disable — MUST use `on_disk_filename` (a disabled plugin lives
  // at `<name>.jar.disabled`), never the base display filename.
  async function toggle(row: ServerRow) {
    actionError = null;
    const res = row.card.installed.enabled
      ? await commands.serverDisablePlugin(serverId, row.onDiskFilename)
      : await commands.serverEnablePlugin(serverId, row.onDiskFilename);
    if (res.status === 'ok') await data.refresh();
    else actionError = formatError(res.error);
  }

  async function confirmDelete(row: ServerRow) {
    actionError = null;
    deleting = true;
    try {
      const res = await commands.serverDeletePlugin(serverId, row.onDiskFilename);
      if (res.status === 'ok') {
        pendingDelete = null;
        await data.refresh();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      deleting = false;
    }
  }

  async function openFolder() {
    busyFolder = true;
    try {
      const res = await commands.serverOpenPluginsFolder(serverId);
      if (res.status !== 'ok') {
        actionError = formatError(res.error);
      }
    } finally {
      busyFolder = false;
    }
  }
</script>

{#if !isPluginCore}
  <p class="text-sm text-secondary">{$t('servers.plugins.requiresCore')}</p>
{:else}
  <div class="flex flex-col gap-3">
    <!-- Toolbar: folder only — local/browser installs are owned by the
         Add-ons host (tab-level dropzone + Browse sub-tab). -->
    <div class="flex flex-wrap items-center gap-2">
      <BusyButton class="btn-secondary btn-sm" busy={busyFolder} onclick={() => void openFolder()}>
        <Icon name="folderOpen" size={14} />
        {$t('servers.plugins.openFolder')}
      </BusyButton>
      <BusyButton
        class="btn-secondary btn-sm"
        data-testid="server-plugins-check-updates"
        busy={checkingUpdates}
        disabled={!canManage}
        onclick={() => void checkUpdates()}
      >
        {$t('servers.plugins.checkUpdates')}
      </BusyButton>
      <BusyButton
        class="btn-warning btn-sm"
        data-testid="server-plugins-update-all"
        busy={updatingAll}
        disabled={!canManage || updatableCount === 0}
        onclick={() => void updateAll()}
      >
        {$t('mods.installed.updateAll', { count: updatableCount })}
      </BusyButton>
    </div>

    {#if isRunning}
      <p class="text-xs text-warning-text">{$t('servers.plugins.stopToManage')}</p>
    {/if}

    <!-- Note -->
    <p class="text-xs text-secondary">{$t('servers.plugins.note')}</p>

    {#if data.error}
      <p class="text-sm text-danger">{data.error}</p>
    {/if}
    {#if actionError}
      <p class="text-sm text-danger">{actionError}</p>
    {/if}

    {#if data.rows.length === 0 && !data.error && !data.loading}
      <p class="text-sm text-muted">{$t('servers.plugins.empty')}</p>
    {:else if data.rows.length > 0}
      <!-- Filter toolbar: search + all/enabled/disabled + sort. Gated on
           data.rows so an empty search still shows the controls. -->
      <div class="flex flex-wrap items-center gap-2">
        <input
          type="search"
          placeholder={$t('mods.installed.filterPlaceholder')}
          aria-label={$t('mods.installed.filterPlaceholder')}
          class="min-w-40 flex-1 rounded border border-border-emphasis px-3 py-1.5 text-sm"
          bind:value={filters.filter}
        />
        <div class="inline-flex items-center gap-1 text-xs text-secondary">
          {$t('mods.installed.sortLabel')}
          <Select
            class="text-xs"
            ariaLabel={$t('mods.installed.sortLabel')}
            value={filters.sortBy}
            options={sortOptions}
            onChange={(v) => (filters.sortBy = String(v) as SortBy)}
          />
        </div>
      </div>
      <ToggleChipGroup
        options={filterOptions}
        value={filters.viewFilter}
        onChange={(v) => (filters.viewFilter = v as ViewFilter)}
        ariaLabel={$t('mods.installed.filterGroupAriaLabel')}
      />

      {#if filters.filtered.length === 0}
        <p class="text-sm text-muted">{$t('servers.plugins.noResults')}</p>
      {:else}
        <div class="flex flex-col gap-2">
          {#each filters.filtered as row (row.sha1)}
            <ServerInstalledRow
              card={row.card}
              canToggle={canManage}
              checking={checkingUpdates}
              updateState={canManage ? (updateChecks.get(row.sha1) ?? null) : null}
              onOpenDetail={() => {
                if (row.card.summary) detail = row.card.summary;
              }}
              onUpdate={() => void updateOne(row)}
              onToggle={() => void toggle(row)}
              onUninstall={() => {
                // ModCard's trash button can't be gated per-row (no prop for it),
                // so refuse here while running instead of opening a dialog whose
                // confirm the backend would only reject.
                if (isRunning) {
                  actionError = $t('servers.plugins.stopToManage');
                  return;
                }
                actionError = null;
                pendingDelete = row;
              }}
            />
          {/each}
        </div>
      {/if}
    {/if}

    {#if pendingDelete}
      <ConfirmDialog
        title={$t('servers.plugins.delete')}
        bodyText={$t('servers.plugins.deleteConfirm', { name: pendingDelete.card.installed.name })}
        confirmLabel={$t('servers.plugins.delete')}
        variant="danger"
        busy={deleting}
        error={actionError}
        onCancel={() => (pendingDelete = null)}
        onConfirm={() => pendingDelete && void confirmDelete(pendingDelete)}
      />
    {/if}

    {#if detail && server}
      {@const d = detail}
      <ServerContentDetail
        project={d}
        onClose={() => (detail = null)}
        loadProject={() => commands.modsProject(d.source, d.project_id)}
        loadVersions={() =>
          commands.modsPluginVersions(d.source, d.project_id, server.mc_version, server.loader)}
        installVersion={(vid) =>
          commands.serverInstallPlugin(serverId, d.source, d.project_id, vid)}
        externalOf={(v) => externalOf(d, v)}
        openExternal={openUrl}
        projectUrl={modProjectUrl(d.source, d.slug ?? d.project_id, d.author)}
        onInstalled={() => {
          detail = null;
          void data.refresh();
        }}
      />
    {/if}
  </div>
{/if}
