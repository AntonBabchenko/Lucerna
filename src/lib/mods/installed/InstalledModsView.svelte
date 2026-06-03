<script lang="ts">
  import {
    commands,
    events,
    type LoaderKind,
    type ModSource,
    type ModVersion,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { settingsOpen } from '$lib/settings/state.svelte';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { get } from 'svelte/store';
  import { onDestroy, onMount } from 'svelte';
  import CurseForgeKeyBanner from '../CurseForgeKeyBanner.svelte';
  import ModDetailModal from '../ModDetailModal.svelte';
  import OrphanUninstallDialog from '../OrphanUninstallDialog.svelte';
  import PageSizePicker from '../PageSizePicker.svelte';
  import { browserPrefs } from '../browser-prefs.svelte';
  import { createInstalledData, type Row } from './installed-data.svelte';
  import { createInstalledFilters } from './installed-filters.svelte';
  import { createUpdateCheck } from './update-check.svelte';
  import { createDepGraph } from './dep-graph.svelte';
  import { createInstalledSelection } from './installed-selection.svelte';
  import { modKey } from './row-utils';
  import InstalledToolbar from './InstalledToolbar.svelte';
  import BulkActionBar from './BulkActionBar.svelte';
  import InstalledModRow from './InstalledModRow.svelte';

  let {
    instanceId,
    mcVersion,
    loader,
  }: {
    instanceId: string | null;
    mcVersion: string | null;
    loader: LoaderKind | null;
  } = $props();

  // --- composables (creation order matters; thunks keep cross-refs lazy) ---
  const data = createInstalledData(() => instanceId);
  const updates = createUpdateCheck(() => instanceId, data.refresh);
  const filters = createInstalledFilters(
    () => data.rows,
    () => updates.updatableShas,
    () => deps.missingShas,
  );
  const deps = createDepGraph(
    () => instanceId,
    () => data.rows,
    {
      getMcVersion: () => mcVersion,
      getLoader: () => loader,
      refresh: data.refresh,
      getFiltered: () => filters.filtered,
      setPage: (n) => (filters.page = n),
      getPageSize: () => filters.pageSize,
    },
  );
  const selection = createInstalledSelection(
    () => filters.filtered,
    () => instanceId,
    data.refresh,
    () => updates.updateChecks,
    deps.invalidateGraph,
  );

  // Independent per-instance page size, persisted under its own key.
  $effect(() => {
    filters.pageSize = browserPrefs.installedPageSize;
  });

  // Single-row ops (toggle/uninstall/switchVersion) live in the shell, so they
  // need their own busy flag folded into the aggregate — otherwise the toolbar
  // and bulk bar stay clickable mid-IPC (the monolith gated them via `busy`).
  let shellBusy = $state(false);

  const busy = $derived(shellBusy || selection.busy || deps.busy || updates.busy);
  const error = $derived(data.error ?? deps.error ?? updates.error ?? selection.error);

  // Version-switch drawer (bridges ModDetailModal + data; lives in the shell).
  let drawerRow = $state<Row | null>(null);
  async function switchVersion(row: Row, v: ModVersion) {
    if (!instanceId) return;
    drawerRow = null;
    data.error = null;
    shellBusy = true;
    const removed = await commands.modsUninstall(instanceId, row.installed.sha1);
    if (removed.status === 'error') {
      data.error = formatError(removed.error);
      shellBusy = false;
      return;
    }
    const installed = await commands.modsInstallWithDeps(
      instanceId,
      { source: v.source, project_id: v.project_id, version_id: v.version_id },
      [],
    );
    if (installed.status === 'error') {
      pushWarning(get(t)('mods.browse.toastInstallFailed'), [formatError(installed.error)]);
    } else {
      pushSuccess(
        get(t)('mods.browse.toastInstalledMod', { name: row.summary?.name ?? row.installed.name }),
      );
    }
    await data.refresh();
    shellBusy = false;
  }

  async function toggle(m: Row['installed']) {
    if (!instanceId) return;
    data.error = null;
    shellBusy = true;
    const result = m.enabled
      ? await commands.modsDisable(instanceId, m.sha1)
      : await commands.modsEnable(instanceId, m.sha1);
    if (result.status === 'error') data.error = formatError(result.error);
    else await data.refresh();
    shellBusy = false;
  }
  async function uninstall(m: Row['installed']) {
    if (!instanceId) return;
    data.error = null;
    shellBusy = true;
    const result = await commands.modsUninstall(instanceId, m.sha1);
    if (result.status === 'error') data.error = formatError(result.error);
    else {
      await data.refresh();
      deps.reloadGraph();
    }
    shellBusy = false;
  }

  // Bulk update: apply, then clear the now-stale update-check state so badges
  // don't linger (the selection composable owns the update IPC but not the
  // update-check cache, which lives in the update-check composable).
  async function bulkUpdate() {
    await selection.bulkUpdate();
    updates.clearChecks();
  }

  // Event listeners (belt-and-suspenders; also call refresh directly).
  let unlisteners: Array<() => void> = [];
  onMount(async () => {
    const handlers = [
      events.modInstalled.listen(() => {
        void data.refresh();
        deps.reloadGraph();
      }),
      events.modUninstalled.listen(() => {
        void data.refresh();
        deps.reloadGraph();
      }),
      events.modToggle.listen(() => void data.refresh()),
    ];
    for (const p of handlers) unlisteners.push(await p);
  });
  onDestroy(() => {
    for (const u of unlisteners) u();
    unlisteners = [];
    data.dispose();
    filters.dispose();
    updates.dispose();
    deps.dispose();
    selection.dispose();
  });
</script>

<div class="p-3">
  <InstalledToolbar
    counts={filters.counts}
    bind:filter={filters.filter}
    bind:sortBy={filters.sortBy}
    bind:enabledFilter={filters.enabledFilter}
    {busy}
    checking={updates.checking}
    graphLoading={deps.graphLoading}
    updateCount={updates.updateCount}
    onCheckUpdates={updates.checkUpdates}
    onRecheckDeps={deps.recheckDeps}
    onUpdateAll={updates.updateAll}
  />

  {#if error}
    <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2 mb-2">
      {error}
    </div>
  {/if}
  {#if updates.showCfBanner}
    <CurseForgeKeyBanner onOpenSettings={() => (settingsOpen.value = { tab: 'curseforge' })} />
  {/if}

  {#if !instanceId}
    <div class="text-placeholder text-sm py-8 text-center">
      {$t('mods.installed.pickInstanceFirst')}
    </div>
  {:else if data.loading && data.rows.length === 0}
    <div class="text-placeholder text-sm py-8 text-center">{$t('mods.installed.loading')}</div>
  {:else if data.rows.length === 0}
    <div class="text-placeholder text-sm py-8 text-center">{$t('mods.installed.empty')}</div>
  {:else}
    <div class="border border-border-subtle rounded overflow-hidden">
      <BulkActionBar
        allSelected={selection.allSelected}
        selectedCount={selection.selected.size}
        indeterminate={selection.selected.size > 0 && !selection.allSelected}
        {busy}
        canUpdate={selection.selectedUpdatable.length > 0}
        onToggleAll={selection.toggleSelectAll}
        onEnable={() => selection.bulkSetEnabled(true)}
        onDisable={() => selection.bulkSetEnabled(false)}
        onUpdate={bulkUpdate}
        onUninstall={selection.requestBulkUninstall}
        onClear={selection.clear}
      />
      {#each filters.paged as row (row.installed.sha1)}
        {@const rowKey = modKey(row.installed.source, row.installed.project_id, row.installed.sha1)}
        {@const root = deps.rootBySha.get(row.installed.sha1)}
        {@const counts = deps.depCounts(root)}
        {@const reqBy = deps.requiredBy.get(row.installed.project_id ?? '') ?? []}
        <InstalledModRow
          summary={row.summary}
          installed={row.installed}
          {rowKey}
          {root}
          requiredByNames={reqBy}
          depTotal={counts.total}
          depMissing={counts.missing}
          expanded={deps.expanded.has(row.installed.sha1)}
          graphLoading={deps.graphLoading}
          hoveredKey={deps.hoveredKey}
          updateState={updates.updateChecks.get(row.installed.sha1)?.state ?? null}
          checking={updates.checking}
          packChip={data.packSummary && data.packSummary.mod_shas.includes(row.installed.sha1)
            ? data.packSummary.project_name
            : null}
          selected={selection.selected.has(row.installed.sha1)}
          onToggleExpand={() => deps.toggleExpand(row.installed.sha1)}
          onHover={(k) => (deps.hoveredKey = k)}
          onOpenDetail={() => (drawerRow = row)}
          onToggle={() => toggle(row.installed)}
          onUninstall={() => uninstall(row.installed)}
          onUpdate={() => updates.updateOne(row.installed)}
          onSelectChange={(c) => selection.toggleSelect(row.installed.sha1, c)}
          onInstallDep={deps.installDepNode}
          onJump={deps.jumpToMod}
        />
      {/each}
    </div>

    <!-- Pagination footer — unified with Browse/Modpacks (Steam-style). -->
    <div class="flex items-center gap-3 text-sm text-secondary pt-2">
      <span class="flex-1"></span>
      <button
        type="button"
        class="btn-secondary btn-sm"
        disabled={filters.page === 0}
        onclick={() => (filters.page = Math.max(0, filters.page - 1))}
      >
        {$t('mods.browse.prev')}
      </button>
      <span>{$t('mods.browse.pageOf', { page: filters.page + 1, total: filters.pageCount })}</span>
      <button
        type="button"
        class="btn-secondary btn-sm"
        disabled={filters.page >= filters.pageCount - 1}
        onclick={() => (filters.page = Math.min(filters.pageCount - 1, filters.page + 1))}
      >
        {$t('mods.browse.next')}
      </button>
      <span class="flex-1 flex justify-end"><PageSizePicker prefsKey="installedPageSize" /></span>
    </div>
  {/if}

  {#if drawerRow && drawerRow.installed.source && drawerRow.installed.project_id && instanceId}
    <ModDetailModal
      source={drawerRow.installed.source as ModSource}
      projectId={drawerRow.installed.project_id}
      {mcVersion}
      {loader}
      installedVersionId={drawerRow.installed.version_id}
      onClose={() => (drawerRow = null)}
      onInstall={(v) => {
        if (drawerRow) void switchVersion(drawerRow, v);
      }}
    />
  {/if}

  {#if selection.uninstallPrompt}
    <OrphanUninstallDialog
      removingNames={selection.uninstallPrompt.names}
      orphans={selection.uninstallPrompt.orphans}
      onCancel={selection.cancelUninstall}
      onConfirm={selection.confirmBulkUninstall}
    />
  {/if}
</div>
