<script lang="ts">
  import { get } from 'svelte/store';
  import { onDestroy, type Snippet } from 'svelte';
  import {
    commands,
    type ModSummary,
    type ModUpdateState,
    type ModVersion,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { modProjectUrl } from '$lib/mods/project-url';
  import { coreToLoaderKind, modCapable, pluginCapable } from '$lib/servers/core-display';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import BulkActionBar, { type BulkBarAction } from '$lib/ui/BulkActionBar.svelte';
  import { bulkNameLines, reportBulk, runBulk } from '$lib/ui/bulk-run';
  import { createListSelection } from '$lib/ui/list-selection.svelte';
  import { refocusAfterRemoval } from '$lib/ui/refocus-after-removal';
  import { Icon } from '$lib/ui/icons';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import Select from '$lib/ui/Select.svelte';
  import ToggleChipGroup from '$lib/ui/ToggleChipGroup.svelte';
  import { openExternalHttps } from '$lib/ui/safe-open';
  import ServerContentDetail from '$lib/servers/browser/ServerContentDetail.svelte';
  import {
    createInstalledFilters,
    type SortBy,
    type ViewFilter,
  } from '$lib/mods/installed/installed-filters.svelte';
  import ServerInstalledRow from './ServerInstalledRow.svelte';
  import { createServerInstalledData, type ServerRow } from './server-installed-data.svelte';

  let {
    serverId,
    reloadToken = 0,
    emptyDropzone,
    onEmptyChange = () => {},
  }: {
    serverId: string;
    reloadToken?: number;
    /** The host's full drop area, rendered in the empty list (passed only while it shows). */
    emptyDropzone?: Snippet;
    /** Loaded and empty — the host hides its strip meanwhile (DESIGN.md §14). */
    onEmptyChange?: (empty: boolean) => void;
  } = $props();

  // Enriched Installed list (enrich → ModSummary resolution → ServerRow[]). The
  // composable owns the list + reload-token effect and blanks on server switch.
  const data = createServerInstalledData(
    () => serverId,
    'mod',
    () => reloadToken,
  );

  let actionError = $state<string | null>(null);
  let busyFolder = $state(false);
  let busyQuarantine = $state(false);
  // Pane-level delete confirm (ServerInstalledRow delegates delete up).
  let pendingDelete = $state<ServerRow | null>(null);
  let deleting = $state(false);
  // The bulk bar's action in flight (drives its one spinner), null = none.
  type BulkAction = 'enable' | 'disable' | 'update' | 'remove';
  let bulkAction = $state<BulkAction | null>(null);
  // The rows a bulk Remove was asked for (the dialog's model), null = closed.
  let pendingBulkDelete = $state<ServerRow[] | null>(null);
  // The rows column and the empty list: where focus goes after a bulk removal.
  let listEl = $state<HTMLElement | null>(null);
  let emptyListEl = $state<HTMLElement | null>(null);
  // The enriched project whose in-launcher detail card is open (null = closed).
  // Only identity-bearing rows (card.summary != null) can open it.
  let detail = $state<ModSummary | null>(null);

  // For the detail modal: a version whose file is not distributable must open
  // the project page, never download (mirrors ServerModBrowser).
  function externalOf(card: ModSummary, v: ModVersion): string | null {
    if (v.primary_file.distribution_allowed) return null;
    return modProjectUrl(card.source, card.slug ?? card.project_id, card.author);
  }

  // Every URL leaves through the one https-only chokepoint ($lib/ui/safe-open).
  function openUrl(url: string): void {
    void openExternalHttps(url);
  }

  // Per-mod update-check results, keyed by sha1 (identity that survives an
  // enable/disable rename). Plugins are out of scope — this pane is mods-only.
  let updateChecks = $state(new Map<string, ModUpdateState>());
  let checkingUpdates = $state(false);
  // Per-row in-flight guard: `serverUpdateOne` is a filesystem-mutating swap
  // with no backend concurrent-same-mod guard, and the Update icon has no busy
  // state — so a double-click would fire two concurrent updates. Keyed by sha1.
  let updatingShas = $state(new Set<string>());
  // "Update all" runs the pending updates as one sequential batch (see below).
  let updatingAll = $state(false);

  // A different server's checks must never bleed across a switch (sha1 keys can
  // collide across servers). Reset on serverId change.
  $effect(() => {
    void serverId;
    updateChecks = new Map();
  });

  // Search / enabled-disabled / sort over the installed list, hosted on the
  // shared client composable. Server rows carry no install timestamp, so the
  // sort is name-only (sortKey='' → 'recent' would collapse to input order).
  // The `isUpdatable` predicate lights the Updates chip after a check-updates
  // scan; the issues and dependency-graph views have no server equivalent (no
  // predicate).
  const filters = createInstalledFilters(
    () => data.rows,
    (r) => ({
      id: r.sha1,
      name: r.card.summary?.name ?? r.card.installed.name,
      enabled: r.card.installed.enabled,
      sortKey: '',
      source: r.card.installed.source,
    }),
    { isUpdatable: (id) => updateChecks.get(id)?.kind === 'update_available' },
  );
  // The bulk bar's selection: over the filtered rows (keyed by sha1, the identity that survives
  // an enable/disable rename), cleared on a server switch.
  const selection = createListSelection(
    () => filters.filtered.map((r) => r.sha1),
    () => serverId,
  );
  onDestroy(() => {
    data.dispose();
    filters.dispose();
    selection.dispose();
  });

  const sortOptions = $derived([
    { value: 'name-asc', label: $t('mods.installed.sortNameAsc') },
    { value: 'name-desc', label: $t('mods.installed.sortNameDesc') },
  ]);

  // All / Enabled / Disabled are always present; Updates appears only once a
  // check-updates scan has flagged at least one pending update.
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

  // The server's own metadata drives mod applicability. Mods only attach to a
  // mod loader; a vanilla server gets datapacks only. Plugin cores (paper/purpur)
  // have no mod loader either — they get datapacks + plugins, not mods.
  // Mutations require a stopped server (the backend enforces it; the UI
  // gates to avoid pointless errors).
  const server = $derived(serverState.list.find((s) => s.id === serverId) ?? null);
  const isModCapable = $derived(server !== null && modCapable(server.loader));
  const isPluginCore = $derived(server !== null && pluginCapable(server.loader));
  const isRunning = $derived(server?.running ?? false);
  const canManageMods = $derived(server !== null && isModCapable && !isRunning);

  // Empty is reported, never assumed: a list still loading or one that could not be read is not
  // empty — the host keeps its strip until the list says it is empty.
  const listEmpty = $derived(
    !isPluginCore && !data.loading && data.rows.length === 0 && !data.error,
  );
  $effect(() => {
    onEmptyChange(listEmpty);
    return () => onEmptyChange(false);
  });

  // How many installed mods have a pending update, derived from the live rows so
  // a stale check for a since-removed mod never counts. Drives the "Update all"
  // label + enablement.
  const updatableCount = $derived(
    data.rows.filter((row) => updateChecks.get(row.sha1)?.kind === 'update_available').length,
  );

  // The bulk bar (DESIGN.md §9) over the selected rows. What each action applies to: rows
  // already in the wanted state are left alone.
  const selectedRows = $derived(filters.filtered.filter((r) => selection.selected.has(r.sha1)));
  const bulkToEnable = $derived(selectedRows.filter((r) => !r.card.installed.enabled));
  const bulkToDisable = $derived(selectedRows.filter((r) => r.card.installed.enabled));
  const bulkToUpdate = $derived(
    selectedRows.flatMap((r) => {
      const st = updateChecks.get(r.sha1);
      return st?.kind === 'update_available'
        ? [{ sha: r.sha1, name: r.card.installed.name, target: st.target }]
        : [];
    }),
  );
  // A bulk run and any other writer of the mods folder never overlap: the bar waits for a row's
  // update, «Update all», a check and the quarantine, and they (and the rows) wait for the bar.
  const bulkBusy = $derived(
    bulkAction !== null ||
      deleting ||
      updatingAll ||
      checkingUpdates ||
      busyQuarantine ||
      updatingShas.size > 0,
  );
  // Every action is off while the server runs (the mods folder is the server's then); otherwise
  // an action is off when no selected row can take it, and says so.
  function bulkGate(action: BulkBarAction, none: boolean, noneReason: string): BulkBarAction {
    if (!canManageMods)
      return { ...action, disabled: true, disabledReason: $t('servers.mods.stopToManage') };
    return { ...action, disabled: none, disabledReason: noneReason };
  }
  const bulkActions = $derived<BulkBarAction[]>([
    bulkGate(
      { id: 'enable', label: $t('mods.card.enable') },
      bulkToEnable.length === 0,
      $t('ui.bulk.noneApplicable'),
    ),
    bulkGate(
      { id: 'disable', label: $t('mods.card.disable') },
      bulkToDisable.length === 0,
      $t('ui.bulk.noneApplicable'),
    ),
    bulkGate(
      { id: 'update', label: $t('mods.card.update') },
      bulkToUpdate.length === 0,
      $t('ui.bulk.updateNeedsCheck'),
    ),
    bulkGate(
      { id: 'remove', label: $t('servers.mods.delete'), intent: 'danger' },
      selectedRows.length === 0,
      $t('ui.bulk.noneApplicable'),
    ),
  ]);

  function onBulkAction(id: string): void {
    if (id === 'enable') void bulkSetEnabled(true);
    else if (id === 'disable') void bulkSetEnabled(false);
    else if (id === 'update') void bulkUpdate();
    else if (id === 'remove') requestBulkDelete();
  }

  // Toggle enable/disable — MUST use `on_disk_filename` (a disabled mod lives at
  // `<name>.jar.disabled`), never the base display filename.
  async function toggle(row: ServerRow) {
    actionError = null;
    const res = row.card.installed.enabled
      ? await commands.serverDisableMod(serverId, row.onDiskFilename)
      : await commands.serverEnableMod(serverId, row.onDiskFilename);
    if (res.status === 'ok') await data.refresh();
    else actionError = formatError(res.error);
  }

  // Read-only scan: classify every identity-bearing mod against its platform.
  async function checkUpdates() {
    checkingUpdates = true;
    actionError = null;
    try {
      const res = await commands.serverCheckModUpdates(serverId);
      if (res.status === 'ok') {
        const m = new Map<string, ModUpdateState>();
        for (const c of res.data) m.set(c.sha1, c.state);
        updateChecks = m;
      } else actionError = formatError(res.error);
    } finally {
      checkingUpdates = false;
    }
  }

  // Apply one pending update, then drop its check and re-list (the row's sha1
  // and version change after the swap, so the stale entry must go).
  async function updateOne(row: ServerRow) {
    const state = updateChecks.get(row.sha1);
    if (state?.kind !== 'update_available') return;
    // A stale badge can survive a server start (the pane stays mounted and
    // updateChecks only resets on serverId change) — refuse the swap the
    // backend would reject anyway, with a clear message.
    if (!canManageMods) {
      actionError = $t('servers.mods.stopToManage');
      return;
    }
    // No-op the second concurrent click on the same row.
    if (updatingShas.has(row.sha1)) return;
    updatingShas = new Set(updatingShas).add(row.sha1);
    actionError = null;
    try {
      const res = await commands.serverUpdateOne(serverId, row.sha1, state.target);
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

  // Apply every pending update in one sequential batch. Each `serverUpdateOne`
  // swaps the on-disk jar + registry row, so the applies MUST stay serial —
  // parallel calls would race the same mods directory — and the snapshot is
  // taken up front because the checks map goes stale as each swap lands.
  async function updateAll() {
    if (!canManageMods) {
      actionError = $t('servers.mods.stopToManage');
      return;
    }
    const targets = data.rows
      .map((row) => ({ sha: row.sha1, state: updateChecks.get(row.sha1) }))
      .filter((x) => x.state?.kind === 'update_available')
      .map((x) => ({
        sha: x.sha,
        target: (x.state as Extract<ModUpdateState, { kind: 'update_available' }>).target,
      }));
    if (targets.length === 0) return;
    actionError = null;
    updatingAll = true;
    try {
      for (const { sha, target } of targets) {
        const res = await commands.serverUpdateOne(serverId, sha, target);
        if (res.status === 'error') {
          actionError = formatError(res.error);
          break;
        }
      }
      updateChecks = new Map(); // all applied entries are now stale
      await data.refresh();
    } finally {
      updatingAll = false;
    }
  }

  async function confirmDelete(row: ServerRow) {
    actionError = null;
    deleting = true;
    try {
      const res = await commands.serverDeleteMod(serverId, row.onDiskFilename);
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

  // The selected rows that need the flip, one command each with the ON-DISK filename, then one
  // notice. The server id is captured: a switch mid-run must not redirect the rest of the loop.
  async function bulkSetEnabled(enable: boolean): Promise<void> {
    const id = serverId;
    const targets = enable ? bulkToEnable : bulkToDisable;
    if (!canManageMods || targets.length === 0) return;
    bulkAction = enable ? 'enable' : 'disable';
    actionError = null;
    try {
      const outcome = await runBulk(
        targets,
        (r) =>
          enable
            ? commands.serverEnableMod(id, r.onDiskFilename)
            : commands.serverDisableMod(id, r.onDiskFilename),
        formatError,
        (r) => r.card.installed.name,
      );
      reportBulk(
        outcome,
        enable
          ? { done: 'ui.bulk.enabled', partial: 'ui.bulk.enabledFailed' }
          : { done: 'ui.bulk.disabled', partial: 'ui.bulk.disabledFailed' },
      );
    } finally {
      bulkAction = null;
    }
    selection.clear();
    await data.refresh();
  }

  // The selected rows with a pending update, serially (each swap writes the mods directory);
  // unlike «Update all», a failure does not stop the run — it is counted and named.
  async function bulkUpdate(): Promise<void> {
    const id = serverId;
    const targets = bulkToUpdate;
    if (!canManageMods || targets.length === 0) return;
    bulkAction = 'update';
    actionError = null;
    try {
      const outcome = await runBulk(
        targets,
        (tg) => commands.serverUpdateOne(id, tg.sha, tg.target),
        formatError,
        (tg) => tg.name,
        (tg) => {
          const next = new Map(updateChecks);
          next.delete(tg.sha);
          updateChecks = next;
        },
      );
      reportBulk(outcome, { done: 'ui.bulk.updated', partial: 'ui.bulk.updatedFailed' });
    } finally {
      bulkAction = null;
    }
    selection.clear();
    await data.refresh();
  }

  function requestBulkDelete(): void {
    if (!canManageMods || selectedRows.length === 0) return;
    actionError = null;
    pendingBulkDelete = selectedRows;
  }

  // Deletes each by on-disk name, then moves focus to the row now in the first removed row's
  // place (DESIGN.md: focus survives a removal).
  async function confirmBulkDelete(): Promise<void> {
    const id = serverId;
    const rows = pendingBulkDelete;
    if (!rows) return;
    const index = Math.max(
      0,
      filters.filtered.findIndex((r) => selection.selected.has(r.sha1)),
    );
    bulkAction = 'remove';
    try {
      const outcome = await runBulk(
        rows,
        (r) => commands.serverDeleteMod(id, r.onDiskFilename),
        formatError,
        (r) => r.card.installed.name,
      );
      pendingBulkDelete = null;
      reportBulk(outcome, { done: 'ui.bulk.removed', partial: 'ui.bulk.removedFailed' });
    } finally {
      bulkAction = null;
    }
    selection.clear();
    await data.refresh();
    await refocusAfterRemoval({
      index,
      listEl: () => listEl,
      rows: (list) => [...list.querySelectorAll<HTMLElement>('[data-bulk-row]')],
      emptyEl: () => emptyListEl,
    });
  }

  async function quarantineClientMods() {
    busyQuarantine = true;
    actionError = null;
    try {
      const r = await serverState.quarantineClientMods(serverId);
      if (r.ok) {
        const n = r.report.disabled.length;
        const kept = r.report.kept_because_required.length;
        let msg =
          n > 0
            ? get(t)('servers.diagnose.quarantined', { count: n })
            : get(t)('servers.diagnose.quarantineNone');
        if (kept > 0) {
          msg += ` ${get(t)('servers.diagnose.quarantineKeptRequired', { count: kept })}`;
        }
        pushSuccess(msg);
        await data.refresh();
      } else {
        actionError = formatError(r.error);
      }
    } finally {
      busyQuarantine = false;
    }
  }

  async function openFolder() {
    busyFolder = true;
    try {
      const res = await commands.serverOpenFolder(serverId);
      if (res.status !== 'ok') {
        actionError = formatError(res.error);
      }
    } finally {
      busyFolder = false;
    }
  }
</script>

<div class="flex flex-col gap-3">
  <!-- Toolbar: folder + quarantine only — local/browser installs are owned by
       the Add-ons host (tab-level dropzone + Browse sub-tab). -->
  <div class="flex flex-wrap items-center gap-2">
    {#if !isPluginCore}
      <BusyButton class="btn-secondary btn-sm" busy={busyFolder} onclick={() => void openFolder()}>
        <Icon name="folderOpen" size={14} />
        {$t('servers.mods.openFolder')}
      </BusyButton>
    {/if}
    {#if isModCapable}
      <BusyButton
        class="btn-secondary btn-sm"
        data-testid="server-mods-check-updates"
        busy={checkingUpdates}
        disabled={!canManageMods || bulkAction !== null}
        onclick={() => void checkUpdates()}
      >
        {$t('servers.mods.checkUpdates')}
      </BusyButton>
      <BusyButton
        class="btn-warning btn-sm"
        data-testid="server-mods-update-all"
        busy={updatingAll}
        disabled={!canManageMods || bulkAction !== null || updatableCount === 0}
        onclick={() => void updateAll()}
      >
        {$t('mods.installed.updateAll', { count: updatableCount })}
      </BusyButton>
      <BusyButton
        class="btn-secondary btn-sm"
        data-testid="server-mods-quarantine"
        busy={busyQuarantine}
        disabled={!canManageMods || bulkAction !== null}
        onclick={() => void quarantineClientMods()}
      >
        {$t('servers.diagnose.quarantineClientMods')}
      </BusyButton>
    {/if}
  </div>

  {#if isPluginCore}
    <p class="text-xs text-secondary">{$t('servers.mods.pluginCoreHint')}</p>
  {:else}
    {#if isRunning}
      <p class="text-xs text-warning-text">{$t('servers.mods.stopToManage')}</p>
    {/if}

    <!-- Note -->
    <p class="text-xs text-secondary">{$t('servers.mods.note')}</p>

    {#if data.error}
      <p class="text-sm text-danger">{data.error}</p>
    {/if}
    {#if actionError}
      <p class="text-sm text-danger">{actionError}</p>
    {/if}

    {#if data.loading && data.rows.length === 0}
      <LoadingPanel label={$t('mods.installed.loading')} />
    {:else if data.rows.length === 0 && !data.error}
      <!-- The host's full drop area replaces its strip here (DESIGN.md §14). -->
      <div
        class="flex flex-col gap-3 outline-none"
        data-testid="list-empty"
        tabindex="-1"
        bind:this={emptyListEl}
      >
        <p class="text-sm text-muted">{$t('servers.mods.empty')}</p>
        {@render emptyDropzone?.()}
      </div>
    {:else if data.rows.length > 0}
      <!-- Filter toolbar: search + all/enabled/disabled(+updates) + sort. Gated
           on data.rows so an empty search still shows the controls. -->
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
        <p class="text-sm text-muted">{$t('servers.mods.noResults')}</p>
      {:else}
        <div class="flex flex-col gap-2" bind:this={listEl}>
          <BulkActionBar
            allSelected={selection.allSelected}
            indeterminate={selection.indeterminate}
            selectedCount={selection.count}
            busy={bulkBusy}
            busyAction={bulkAction}
            hint={$t('servers.mods.bulkHint')}
            actions={bulkActions}
            onToggleAll={selection.toggleAll}
            onAction={onBulkAction}
            onClear={selection.clear}
          />
          {#each filters.filtered as row (row.sha1)}
            <ServerInstalledRow
              card={row.card}
              reason={row.reason}
              selectable={true}
              selected={selection.selected.has(row.sha1)}
              onSelectChange={(c) => selection.toggle(row.sha1, c)}
              actionsBlockedReason={bulkAction !== null ? $t('ui.bulk.running') : null}
              canToggle={canManageMods}
              checking={checkingUpdates}
              updateState={canManageMods ? (updateChecks.get(row.sha1) ?? null) : null}
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
                  actionError = $t('servers.mods.stopToManage');
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
        title={$t('servers.mods.delete')}
        bodyText={$t('servers.mods.deleteConfirm', { name: pendingDelete.card.installed.name })}
        confirmLabel={$t('servers.mods.delete')}
        variant="danger"
        busy={deleting}
        error={actionError}
        onCancel={() => (pendingDelete = null)}
        onConfirm={() => pendingDelete && void confirmDelete(pendingDelete)}
      />
    {/if}

    <!-- The bulk Remove's confirm: the count in the title, the names (capped) in the body. Its
         failures are reported by the run's one notice, not in the dialog. -->
    {#if pendingBulkDelete}
      <ConfirmDialog
        title={$t('servers.mods.deleteManyConfirm', { count: pendingBulkDelete.length })}
        bodyText={bulkNameLines(pendingBulkDelete.map((r) => r.card.installed.name))}
        confirmLabel={$t('servers.mods.delete')}
        variant="danger"
        busy={bulkAction === 'remove'}
        confirmTestid="server-mods-bulk-delete-confirm"
        onCancel={() => (pendingBulkDelete = null)}
        onConfirm={() => void confirmBulkDelete()}
      />
    {/if}

    {#if detail && server}
      {@const d = detail}
      {@const loaderKind = coreToLoaderKind(server.loader)}
      {#if loaderKind}
        <ServerContentDetail
          project={d}
          onClose={() => (detail = null)}
          loadProject={() => commands.modsProject(d.source, d.project_id)}
          loadVersions={() =>
            commands.modsVersions(d.source, d.project_id, server.mc_version, loaderKind)}
          installVersion={(v) =>
            commands.serverInstallMod(serverId, d.source, d.project_id, v.version_id)}
          externalOf={(v) => externalOf(d, v)}
          openExternal={openUrl}
          projectUrl={modProjectUrl(d.source, d.slug ?? d.project_id, d.author)}
          onInstalled={() => {
            detail = null;
            void data.refresh();
          }}
        />
      {/if}
    {/if}
  {/if}
</div>
