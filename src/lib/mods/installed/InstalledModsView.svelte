<script lang="ts">
  import {
    commands,
    events,
    type DepViolation,
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
  import Pagination from '$lib/ui/Pagination.svelte';
  import { browserPrefs } from '../browser-prefs.svelte';
  import { createInstalledData, type Row } from './installed-data.svelte';
  import { createInstalledFilters } from './installed-filters.svelte';
  import { createUpdateCheck } from './update-check.svelte';
  import { createDepGraph } from './dep-graph.svelte';
  import { createPreflight, toOverlayKeys } from '$lib/mods/preflight.svelte';
  import { createInstalledSelection } from './installed-selection.svelte';
  import PreflightPanel from '$lib/mods/PreflightPanel.svelte';
  import { createCompatCheck } from './compat-check.svelte';
  import { displayLoader } from '$lib/instances/loader-display';
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
  const compat = createCompatCheck(
    () => instanceId,
    () => mcVersion,
    () => loader,
    () => data.rows,
  );
  const filters = createInstalledFilters(
    () => data.rows,
    () => updates.updatableShas,
    () => deps.missingShas,
    () => compat.incompatibleShas,
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
  const preflight = createPreflight(() => instanceId);
  const outOfRangeKeys = $derived(
    toOverlayKeys(preflight.report ?? { violations: [] }),
  );
  const selection = createInstalledSelection(
    () => filters.filtered,
    () => instanceId,
    data.refresh,
    () => updates.updateChecks,
    deps.invalidateGraph,
  );

  // Placeholder for Task 12: one-click provider update from the pre-flight panel.
  // TODO(Task 12): wire remediation — resolve newest compatible version and install.
  const onPreflightUpdate = async (_v: DepViolation): Promise<void> => {};

  // Map a mod's compat hint to a tooltip string (needs the instance loader/mc
  // for interpolation, which the composable does not own).
  function compatTitle(sha1: string): string | null {
    const h = compat.hintFor(sha1);
    if (!h) return null;
    if (h.key === 'loader')
      return get(t)('mods.installed.incompatHintLoader', {
        detected: h.detected,
        loader: loader ? displayLoader(loader) : '',
      });
    return get(t)('mods.installed.incompatHintNoRelease', {
      loader: loader ? displayLoader(loader) : '',
      mc: mcVersion ?? '',
    });
  }

  // Independent per-instance page size, persisted under its own key.
  $effect(() => {
    filters.pageSize = browserPrefs.installedPageSize;
  });

  // Drive the compat pipeline: re-scan whenever the instance / mc / loader
  // changes. The composable owns no self-effect (kept directly unit-testable);
  // this is its single reactive trigger, plus the mod add/remove/toggle handlers
  // in onMount. The composable's generation guard makes a rapid switch supersede
  // any in-flight scan from the previous instance.
  $effect(() => {
    void instanceId;
    void mcVersion;
    void loader;
    void compat.runOfflineScan();
  });

  // Single-row ops (toggle/uninstall/detail install) live in the shell, so they
  // need their own busy flag folded into the aggregate — otherwise the toolbar
  // and bulk bar stay clickable mid-IPC (the monolith gated them via `busy`).
  let shellBusy = $state(false);

  const busy = $derived(shellBusy || selection.busy || deps.busy || updates.busy);
  const error = $derived(data.error ?? deps.error ?? updates.error ?? selection.error);

  // Detail modal can target ANY mod by (source, project_id): the row's own mod,
  // an installed dependency, or a not-yet-installed dependency. Install resolves
  // to a swap when a different version of the same project is already installed,
  // else a fresh install.
  let detail = $state<{ source: ModSource; projectId: string } | null>(null);
  function openDetailMod(source: ModSource, projectId: string) {
    detail = { source, projectId };
  }
  const detailInstalledVersionId = $derived.by(() => {
    if (!detail) return null;
    const r = data.rows.find(
      (x) => x.installed.source === detail!.source && x.installed.project_id === detail!.projectId,
    );
    return r?.installed.version_id ?? null;
  });
  async function installDetailVersion(v: ModVersion) {
    if (!instanceId || !detail) return;
    const existing = data.rows.find(
      (x) => x.installed.source === detail!.source && x.installed.project_id === detail!.projectId,
    );
    detail = null;
    shellBusy = true;
    data.error = null;
    if (existing && existing.installed.version_id !== v.version_id) {
      const removed = await commands.modsUninstall(instanceId, existing.installed.sha1);
      if (removed.status === 'error') {
        data.error = formatError(removed.error);
        shellBusy = false;
        return;
      }
    }
    const res = await commands.modsInstallWithDeps(
      instanceId,
      { source: v.source, project_id: v.project_id, version_id: v.version_id },
      [],
    );
    const name = existing?.summary?.name ?? existing?.installed.name ?? v.name;
    if (res.status === 'error') {
      pushWarning(get(t)('mods.browse.toastInstallFailed'), [formatError(res.error)]);
    } else {
      pushSuccess(get(t)('mods.browse.toastInstalledMod', { name }));
    }
    shellBusy = false;
    deps.invalidateGraph();
    preflight.invalidate();
    await data.refresh();
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
      preflight.invalidate();
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

  // Event listeners (belt-and-suspenders; also call refresh directly). The
  // compat composable's effect only re-runs on instance/mc/loader change, so we
  // also re-scan on mod add/remove/toggle here — otherwise a freshly installed
  // mod's incompatibility chip would not appear until the next instance switch
  // (and the Installed view would disagree with the Overview indicator, which
  // already reacts to these events). The re-scan is cheap (offline) and the
  // auto-confirm step skips shas already decided this session.
  let unlisteners: Array<() => void> = [];
  onMount(async () => {
    const handlers = [
      events.modInstalled.listen(() => {
        void data.refresh();
        deps.reloadGraph();
        preflight.invalidate();
        void compat.runOfflineScan();
      }),
      events.modUninstalled.listen(() => {
        void data.refresh();
        deps.reloadGraph();
        preflight.invalidate();
        void compat.runOfflineScan();
      }),
      events.modToggle.listen(() => {
        void data.refresh();
        preflight.invalidate();
        void compat.runOfflineScan();
      }),
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
    preflight.dispose();
    selection.dispose();
    compat.dispose();
  });
</script>

<div class="p-3">
  <InstalledToolbar
    counts={filters.counts}
    bind:filter={filters.filter}
    bind:sortBy={filters.sortBy}
    bind:viewFilter={filters.viewFilter}
    {busy}
    checking={updates.checking}
    graphLoading={deps.graphLoading}
    updateCount={updates.updateCount}
    onCheckUpdates={updates.checkUpdates}
    onRecheckDeps={deps.recheckDeps}
    onUpdateAll={updates.updateAll}
    checkingCompat={compat.checking}
    onCheckCompat={compat.runLiveCheck}
  />

  {#if error}
    <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2 mb-2">
      {error}
    </div>
  {/if}
  {#if updates.showCfBanner}
    <CurseForgeKeyBanner onOpenSettings={() => (settingsOpen.value = { tab: 'curseforge' })} />
  {/if}

  <PreflightPanel report={preflight.report} onUpdate={onPreflightUpdate} />

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
        busyAction={selection.busyAction}
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
          requiredBy={reqBy}
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
          incompatibleTitle={compat.incompatibleShas.has(row.installed.sha1)
            ? compatTitle(row.installed.sha1)
            : null}
          selected={selection.selected.has(row.installed.sha1)}
          {outOfRangeKeys}
          onToggleExpand={() => deps.toggleExpand(row.installed.sha1)}
          onHover={(k) => (deps.hoveredKey = k)}
          onOpenDetail={() => {
            if (row.installed.source && row.installed.project_id)
              openDetailMod(row.installed.source as ModSource, row.installed.project_id);
          }}
          onOpenDetailMod={openDetailMod}
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
    <Pagination
      page={filters.page}
      pageCount={filters.pageCount}
      onPage={(n) => (filters.page = n)}
    >
      {#snippet end()}
        <PageSizePicker prefsKey="installedPageSize" />
      {/snippet}
    </Pagination>
  {/if}

  {#if detail && instanceId}
    <ModDetailModal
      source={detail.source}
      projectId={detail.projectId}
      {mcVersion}
      {loader}
      installedVersionId={detailInstalledVersionId}
      onClose={() => (detail = null)}
      onInstall={installDetailVersion}
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
