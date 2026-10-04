<script lang="ts">
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
  import { pluginCapable } from '$lib/servers/core-display';
  import { serverState } from '$lib/servers/server-state.svelte';
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
  import {
    autoUpdateTargets,
    countAutoUpdatable,
    externalUpdateUrl,
    hasUpdate,
    isAutoUpdatable,
  } from './plugin-update-actions';

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

  // Enriched Installed list. Plugins carry no quarantine reason (rows' `reason`
  // is always null → ServerInstalledRow shows no badge).
  const data = createServerInstalledData(
    () => serverId,
    'plugin',
    () => reloadToken,
  );

  // Search / enabled-disabled / sort over the installed list, hosted on the
  // shared client composable. Only `isUpdatable` (below) — plugins have no
  // issues or dependency-graph views, so those chips stay at 0. Server rows
  // carry no install timestamp, so the sort is name-only.
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

  // For the detail modal: a version whose file is externally hosted must open
  // its page (or the project page when the file URL is empty), never download
  // (mirrors ServerPluginBrowser's Hangar externalUrl handling).
  function externalOf(card: ModSummary, v: ModVersion): string | null {
    if (v.primary_file.distribution_allowed) return null;
    return v.primary_file.url.length > 0
      ? v.primary_file.url
      : modProjectUrl(card.source, card.slug ?? card.project_id, card.author);
  }

  // Routed through the shared https-only chokepoint: Hangar `externalUrl`
  // values (externalOf / externalUpdateUrl) are author-controlled remote
  // data — see $lib/ui/safe-open.
  function openUrl(url: string): void {
    void openExternalHttps(url);
  }

  // Plugins only attach to plugin-capable cores (paper/purpur) — a mod-loader
  // or vanilla server gets no Plugins UI, just the requiresCore hint below.
  // Mutations require a stopped server (the backend enforces it; the UI gates
  // to avoid pointless errors).
  const server = $derived(serverState.list.find((s) => s.id === serverId) ?? null);
  const isPluginCore = $derived(server !== null && pluginCapable(server.loader));
  const isRunning = $derived(server?.running ?? false);
  const canManage = $derived(isPluginCore && !isRunning);

  // Empty is reported, never assumed: a list still loading or one that could not be read is not
  // empty — the host keeps its strip until the list says it is empty.
  const listEmpty = $derived(
    isPluginCore && !data.loading && data.rows.length === 0 && !data.error,
  );
  $effect(() => {
    onEmptyChange(listEmpty);
    return () => onEmptyChange(false);
  });

  // Non-external pending updates drive the "Update all" label + enablement.
  // External-hosted targets open a page individually and are excluded here.
  const updatableCount = $derived(countAutoUpdatable(data.rows, updateChecks));

  // The bulk bar (DESIGN.md §9) over the selected rows. What each action applies to: rows
  // already in the wanted state are left alone.
  const selectedRows = $derived(filters.filtered.filter((r) => selection.selected.has(r.sha1)));
  const bulkToEnable = $derived(selectedRows.filter((r) => !r.card.installed.enabled));
  const bulkToDisable = $derived(selectedRows.filter((r) => r.card.installed.enabled));
  // Hangar-hosted targets open a page one by one; they are not bulk-updatable.
  const bulkToUpdate = $derived(
    selectedRows.flatMap((r) => {
      const st = updateChecks.get(r.sha1);
      return hasUpdate(st) && isAutoUpdatable(st)
        ? [{ sha: r.sha1, name: r.card.installed.name, target: st.target }]
        : [];
    }),
  );
  const bulkBusy = $derived(bulkAction !== null || deleting || updatingAll || checkingUpdates);
  // Every action is off while the server runs (the plugins folder is the server's then);
  // otherwise an action is off when no selected row can take it, and says so.
  function bulkGate(action: BulkBarAction, none: boolean, noneReason: string): BulkBarAction {
    if (!canManage)
      return { ...action, disabled: true, disabledReason: $t('servers.plugins.stopToManage') };
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
      { id: 'remove', label: $t('servers.plugins.delete'), intent: 'danger' },
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

  // The selected rows that need the flip, one command each with the ON-DISK filename, then one
  // notice. The server id is captured: a switch mid-run must not redirect the rest of the loop.
  async function bulkSetEnabled(enable: boolean): Promise<void> {
    const id = serverId;
    const targets = enable ? bulkToEnable : bulkToDisable;
    if (!canManage || targets.length === 0) return;
    bulkAction = enable ? 'enable' : 'disable';
    actionError = null;
    try {
      const outcome = await runBulk(
        targets,
        (r) =>
          enable
            ? commands.serverEnablePlugin(id, r.onDiskFilename)
            : commands.serverDisablePlugin(id, r.onDiskFilename),
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

  // The selected rows with an auto-updatable pending update, serially (each swap writes the
  // plugins directory); unlike «Update all», a failure does not stop the run — it is counted and
  // named. External-hosted rows stay flagged, as after «Update all».
  async function bulkUpdate(): Promise<void> {
    const id = serverId;
    const targets = bulkToUpdate;
    if (!canManage || targets.length === 0) return;
    bulkAction = 'update';
    actionError = null;
    try {
      const outcome = await runBulk(
        targets,
        (tg) => commands.serverUpdatePluginOne(id, tg.sha, tg.target),
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
    if (!canManage || selectedRows.length === 0) return;
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
        (r) => commands.serverDeletePlugin(id, r.onDiskFilename),
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
      listEl,
      rows: (list) => [...list.querySelectorAll<HTMLElement>('[data-bulk-row]')],
      emptyEl: emptyListEl,
    });
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

    {#if data.loading && data.rows.length === 0}
      <LoadingPanel label={$t('mods.installed.loading')} />
    {:else if data.rows.length === 0 && !data.error}
      <!-- The host's full drop area replaces its strip here (DESIGN.md §14). -->
      <div
        class="flex flex-col gap-3"
        data-testid="list-empty"
        tabindex="-1"
        bind:this={emptyListEl}
      >
        <p class="text-sm text-muted">{$t('servers.plugins.empty')}</p>
        {@render emptyDropzone?.()}
      </div>
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
        <div class="flex flex-col gap-2" bind:this={listEl}>
          <BulkActionBar
            allSelected={selection.allSelected}
            indeterminate={selection.indeterminate}
            selectedCount={selection.count}
            busy={bulkBusy}
            busyAction={bulkAction}
            hint={$t('servers.plugins.bulkHint')}
            actions={bulkActions}
            onToggleAll={selection.toggleAll}
            onAction={onBulkAction}
            onClear={selection.clear}
          />
          {#each filters.filtered as row (row.sha1)}
            <ServerInstalledRow
              card={row.card}
              selectable={true}
              selected={selection.selected.has(row.sha1)}
              onSelectChange={(c) => selection.toggle(row.sha1, c)}
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

    <!-- The bulk Remove's confirm: the count in the title, the names (capped) in the body. Its
         failures are reported by the run's one notice, not in the dialog. -->
    {#if pendingBulkDelete}
      <ConfirmDialog
        title={$t('servers.plugins.deleteManyConfirm', { count: pendingBulkDelete.length })}
        bodyText={bulkNameLines(pendingBulkDelete.map((r) => r.card.installed.name))}
        confirmLabel={$t('servers.plugins.delete')}
        variant="danger"
        busy={bulkAction === 'remove'}
        confirmTestid="server-plugins-bulk-delete-confirm"
        onCancel={() => (pendingBulkDelete = null)}
        onConfirm={() => void confirmBulkDelete()}
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
        installVersion={(v) =>
          commands.serverInstallPlugin(serverId, d.source, d.project_id, v.version_id)}
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
