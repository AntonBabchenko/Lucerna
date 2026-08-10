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
  import { installModWithDeps } from '$lib/tasks/adapters/mod-install';
  import { settingsOpen } from '$lib/settings/state.svelte';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { get } from 'svelte/store';
  import { onDestroy } from 'svelte';
  import { listenUntilDestroyed } from '$lib/ipc/listen';
  import { debounceTrailing } from '$lib/ui/debounce';
  import CurseForgeKeyBanner from '../CurseForgeKeyBanner.svelte';
  import ChangelogModal from '../ChangelogModal.svelte';
  import ModDetailModal from '../ModDetailModal.svelte';
  import OrphanUninstallDialog from '../OrphanUninstallDialog.svelte';
  import PageSizePicker from '../PageSizePicker.svelte';
  import Pagination from '$lib/ui/Pagination.svelte';
  import { browserPrefs } from '../browser-prefs.svelte';
  import { createInstalledData, type Row } from './installed-data.svelte';
  import { createInstalledFilters } from './installed-filters.svelte';
  import { createUpdateCheck } from './update-check.svelte';
  import { createDepGraph } from './dep-graph.svelte';
  import {
    createPreflight,
    installMissing,
    remediatePickedVersion,
    remediateViolation,
    toOverlayKeys,
    violationKey,
  } from '$lib/mods/preflight.svelte';
  import FindAlternativeDialog from '../FindAlternativeDialog.svelte';
  import { modProjectUrl } from '$lib/mods/project-url';
  import { SvelteSet } from 'svelte/reactivity';
  import { createInstalledSelection } from './installed-selection.svelte';
  import PreflightPanel from '$lib/mods/PreflightPanel.svelte';
  import { createCompatCheck } from './compat-check.svelte';
  import { displayLoader } from '$lib/instances/loader-display';
  import { modKey, rowDisplayName } from './row-utils';
  import InstalledToolbar from './InstalledToolbar.svelte';
  import BulkActionBar from './BulkActionBar.svelte';
  import InstalledModRow from './InstalledModRow.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';

  let {
    instanceId,
    mcVersion,
    loader,
    loaderVersion = null,
    requestedFilter = null,
    onFilterApplied = () => {},
    onBrowseFor = (_q: string) => {},
  }: {
    instanceId: string | null;
    mcVersion: string | null;
    loader: LoaderKind | null;
    // Needed to interpolate a platform-loader-axis mismatch hint ("needs loader
    // version X, this profile runs Y"); optional because callers that never hit
    // that hint (tests, other embeddings) should not have to supply it.
    loaderVersion?: string | null;
    // A status view asked for by a deep-link (Overview → "N incompatible
    // mods"). Applied once, then cleared by the parent so an in-tab click is
    // never hijacked afterwards.
    requestedFilter?: 'incompatible' | null;
    onFilterApplied?: () => void;
    onBrowseFor?: (query: string) => void;
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
  // Declared before `filters` because `hasIssue` reads it: the pre-flight is the
  // ONLY source of "this mod is a problem". The graph reports what the platform
  // was told; only the pre-flight reads the descriptor the loader enforces. A
  // mod appears here iff it is the dependent in a violation.
  const preflight = createPreflight(() => instanceId);
  const outOfRangeKeys = $derived(toOverlayKeys(preflight.report ?? { violations: [] }));
  const preflightShas = $derived(
    new Set((preflight.report?.violations ?? []).map((v) => v.dependent_sha1)),
  );
  const filters = createInstalledFilters(
    () => data.rows,
    (r) => ({
      id: r.installed.sha1,
      name: rowDisplayName(r),
      enabled: r.installed.enabled,
      sortKey: r.installed.installed_at,
      source: r.installed.source,
    }),
    {
      isUpdatable: (id) => updates.updatableShas.has(id),
      hasIssue: (id) => preflightShas.has(id),
      isIncompatible: (id) => compat.incompatibleShas.has(id),
    },
    // The list renders before its rows arrive, so a status count of 0 during
    // the initial load is "not known yet", not "none". `refresh()` sets
    // `loading` synchronously before its first await, so this is already true
    // when the filters' auto-reset effect first runs on mount.
    () => !data.loading,
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

  // Per-row pre-flight remediation state, keyed by violationKey. `busy` shows a
  // spinner on the row; `deadEnd` flips it to the no-satisfying affordances
  // (open mod page / find alternative). The picker + find-alternative dialogs
  // are driven by the *Violation holders below.
  let preflightBusy = $state(new SvelteSet<string>());
  let preflightDeadEnd = $state(new SvelteSet<string>());
  let pickerViolation = $state<DepViolation | null>(null);
  let findAltViolation = $state<DepViolation | null>(null);

  // Reset per-row remediation state on instance switch. The keys are dep-based
  // (dependent_sha1:dep_id), not instance-scoped, so a stale busy spinner or
  // dead-end could otherwise bleed onto a same-named dep in another instance.
  $effect(() => {
    void instanceId;
    preflightBusy.clear();
    preflightDeadEnd.clear();
    pickerViolation = null;
    findAltViolation = null;
  });

  async function refreshAfterRemediate(): Promise<void> {
    preflight.invalidate();
    deps.invalidateGraph();
    await data.refresh();
  }

  // Smart one-click update: install the newest version that satisfies the dep
  // range AND the instance MC/loader. On a no-satisfying dead-end the row flips
  // to the open-page / find-alternative affordances instead of a useless retry.
  const onPreflightUpdate = async (v: DepViolation): Promise<void> => {
    if (!instanceId || !mcVersion || !loader) return;
    const key = violationKey(v);
    preflightBusy.add(key);
    // finally clears the busy key even if an IPC call throws (bridge teardown),
    // so a row can never get stuck showing a spinner.
    try {
      const result = await remediateViolation(instanceId, v, mcVersion, loader);
      if (result.ok) {
        preflightDeadEnd.delete(key);
        pushSuccess(
          get(t)('mods.preflight.installedVersion', {
            dep: v.dep_display_name ?? v.dep_id,
            version: result.installedVersion ?? '',
          }),
        );
        await refreshAfterRemediate();
      } else if (result.reason === 'no-satisfying') {
        preflightDeadEnd.add(key);
      } else {
        pushWarning(get(t)('mods.browse.toastInstallFailed'));
      }
    } finally {
      preflightBusy.delete(key);
    }
  };

  // Open the dependency's version list so the user can install any version
  // (including a downgrade) — routed in place via remediatePickedVersion.
  const onPreflightChooseVersion = (v: DepViolation): void => {
    pickerViolation = v;
  };

  // Open the find-alternative search for a dependency with no satisfying version.
  const onPreflightFindAlternative = (v: DepViolation): void => {
    findAltViolation = v;
  };

  // Open the dependency's platform page in the browser.
  const onPreflightOpenModPage = (v: DepViolation): void => {
    const ref = v.provider_project;
    if (!ref) return;
    const slugOrId = ref.source === 'modrinth' ? ref.project_id : String(ref.mod_id);
    void import('@tauri-apps/plugin-opener').then((m) =>
      m.openUrl(modProjectUrl(ref.source, slugOrId)),
    );
  };

  // Install a user-chosen version from the picker (manual pick / downgrade).
  const onPreflightPickInstall = async (chosen: ModVersion): Promise<void> => {
    if (!instanceId || !pickerViolation) return;
    const v = pickerViolation;
    const r = await remediatePickedVersion(instanceId, v, chosen);
    if (r.ok) {
      preflightDeadEnd.delete(violationKey(v));
      pickerViolation = null;
      pushSuccess(
        get(t)('mods.preflight.installedVersion', {
          dep: v.dep_display_name ?? v.dep_id,
          version: r.installedVersion ?? '',
        }),
      );
      await refreshAfterRemediate();
    } else {
      pushWarning(get(t)('mods.browse.toastInstallFailed'));
    }
  };

  // One-click install of a missing required dependency from the pre-flight
  // panel. Resolves the dep by its loader mod-id and installs it; on success
  // the panel + graph refresh. When the dep can't be auto-resolved the helper
  // returns an open_search outcome — hand the query up to the Add-ons shell so
  // it switches to Browse with the search pre-filled.
  const onInstallMissingDep = async (v: DepViolation): Promise<void> => {
    if (!instanceId) return;
    const outcome = await installMissing(instanceId, v.dep_id);
    if (outcome.kind === 'installed') {
      pushSuccess(get(t)('mods.browse.toastInstalledMod', { name: outcome.name }));
      preflight.invalidate();
      deps.invalidateGraph();
      await data.refresh();
    } else {
      pushWarning(
        get(t)('mods.preflight.installSearchFallback', { dep: v.dep_display_name ?? v.dep_id }),
      );
      onBrowseFor(outcome.query);
    }
  };

  // A find-alternative install resolves the original violation (the alternative
  // now provides the dep) — clear its dead-end state and refresh. The dialog
  // shows its own success toast.
  const onPreflightAltInstalled = async (): Promise<void> => {
    if (findAltViolation) preflightDeadEnd.delete(violationKey(findAltViolation));
    findAltViolation = null;
    await refreshAfterRemediate();
  };

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
    if (h.key === 'platformMc')
      return get(t)('mods.installed.incompatHintPlatformMc', {
        declared: h.declared,
        mc: mcVersion ?? '',
      });
    if (h.key === 'platformLoader')
      return get(t)('mods.installed.incompatHintPlatformLoader', {
        declared: h.declared,
        loaderVersion: loaderVersion ?? '',
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

  // Apply a deep-linked status view once. The scan that populates the chip may
  // still be in flight when this lands, so it is applied unconditionally — the
  // composable's existing auto-reset drops back to `all` if the count really
  // is zero, which keeps a stale link from stranding the user on an empty list.
  $effect(() => {
    if (requestedFilter === null) return;
    filters.viewFilter = requestedFilter;
    onFilterApplied();
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

  // Cumulative changelog for a pending mod update. The row exposes the button
  // only when an update is available from a supported source; this builds the
  // (installed → target) request from the update-check result.
  let changelogReq = $state<{
    source: ModSource;
    projectId: string;
    title: string;
    target: string;
    base: string | null;
  } | null>(null);
  function openChangelog(row: Row) {
    const c = updates.updateChecks.get(row.installed.sha1);
    if (!c || c.state.kind !== 'update_available') return;
    const { source, project_id, version_id } = row.installed;
    if (!source || !project_id) return;
    changelogReq = {
      source,
      projectId: project_id,
      title: `${rowDisplayName(row)} ${row.installed.version_number ?? ''} → ${c.state.target.version_number}`,
      target: c.state.target.version_id,
      base: version_id,
    };
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
    const res = await installModWithDeps(
      instanceId,
      existing?.summary?.name ?? existing?.installed.name ?? v.name,
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
  // mod's incompatibility chip would not appear until the next instance switch.
  //
  // `force` is load-bearing, not defensive: the shared store keys on
  // (instance, mc, loader), and none of these events changes that key, so an
  // unforced call is deduplicated away and the re-scan silently does nothing.
  // That is what happened between #332 and this fix — the PR that gave the two
  // surfaces one store also disabled the trigger that kept it fresh.
  //
  // Registration/teardown is race-safe via listenUntilDestroyed (the pattern
  // was born here and is now the shared helper). Handlers are debounced: a
  // with-deps install emits one event per jar, and each un-coalesced event
  // used to trigger a full refresh + preflight resolve + compat scan.
  const debouncedSetChanged = debounceTrailing(() => {
    void data.refresh();
    deps.reloadGraph();
    preflight.invalidate();
    void compat.runOfflineScan({ force: true });
  }, 150);
  const debouncedToggle = debounceTrailing(() => {
    void data.refresh();
    preflight.invalidate();
    void compat.runOfflineScan({ force: true });
  }, 150);
  // Something OTHER than us wrote into mods/. Refresh everything derived from
  // the mod list — and deliberately NOT the list. `mods_list_installed` is what
  // emits this event, so refreshing it here would feed the handler its own
  // trigger. Whatever call produced the event already returned the reconciled
  // list to its caller, so the rows are current without our help.
  const debouncedExternalChange = debounceTrailing(() => {
    deps.reloadGraph();
    preflight.invalidate();
    void compat.runOfflineScan({ force: true });
  }, 150);
  listenUntilDestroyed([
    events.modInstalled.listen(debouncedSetChanged.call),
    events.modUninstalled.listen(debouncedSetChanged.call),
    events.modToggle.listen(debouncedToggle.call),
    events.modsReconciled.listen(debouncedExternalChange.call),
  ]);
  onDestroy(() => {
    debouncedSetChanged.cancel();
    debouncedToggle.cancel();
    debouncedExternalChange.cancel();
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
    <CurseForgeKeyBanner onOpenSettings={() => (settingsOpen.value = { tab: 'integrations' })} />
  {/if}

  <PreflightPanel
    report={preflight.report}
    onUpdate={onPreflightUpdate}
    onInstallMissing={onInstallMissingDep}
    onChooseVersion={onPreflightChooseVersion}
    onFindAlternative={onPreflightFindAlternative}
    onOpenModPage={onPreflightOpenModPage}
    busyKeys={preflightBusy}
    deadEndKeys={preflightDeadEnd}
  />

  {#if !instanceId}
    <div class="text-placeholder text-sm py-8 text-center">
      {$t('mods.installed.pickInstanceFirst')}
    </div>
  {:else if data.loading && data.rows.length === 0}
    <LoadingPanel label={$t('mods.installed.loading')} />
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
          hasPreflightIssue={preflightShas.has(row.installed.sha1)}
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
          onShowChangelog={() => openChangelog(row)}
          onSelectChange={(c) => selection.toggleSelect(row.installed.sha1, c)}
          onInstallDep={deps.installDepNode}
          onJump={deps.jumpToMod}
        />
      {/each}
    </div>

    <!-- Pagination footer — unified with Browse/Modpacks (Steam-style). -->
    <div class="sticky bottom-0 z-10 bg-base border-t border-border-subtle">
      <Pagination
        page={filters.page}
        pageCount={filters.pageCount}
        onPage={(n) => (filters.page = n)}
      >
        {#snippet end()}
          <PageSizePicker prefsKey="installedPageSize" />
        {/snippet}
      </Pagination>
    </div>
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

  {#if changelogReq}
    <ChangelogModal
      source={changelogReq.source}
      projectId={changelogReq.projectId}
      title={changelogReq.title}
      targetVersionId={changelogReq.target}
      baseVersionId={changelogReq.base}
      onClose={() => (changelogReq = null)}
    />
  {/if}

  {#if pickerViolation && pickerViolation.provider_project && instanceId}
    {@const pp = pickerViolation.provider_project}
    <ModDetailModal
      source={pp.source}
      projectId={pp.source === 'modrinth' ? pp.project_id : String(pp.mod_id)}
      kind="mod"
      {mcVersion}
      {loader}
      installedVersionId={null}
      needed={pickerViolation.needed}
      family={pickerViolation.family}
      onClose={() => (pickerViolation = null)}
      onInstall={onPreflightPickInstall}
    />
  {/if}

  {#if findAltViolation && instanceId && mcVersion && loader}
    <FindAlternativeDialog
      modName={findAltViolation.dep_display_name ?? findAltViolation.dep_id}
      {mcVersion}
      {loader}
      {instanceId}
      onClose={() => (findAltViolation = null)}
      onInstalled={onPreflightAltInstalled}
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
