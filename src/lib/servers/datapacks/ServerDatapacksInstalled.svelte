<script lang="ts">
  import { onDestroy, type Snippet, untrack } from 'svelte';
  import {
    commands,
    type AssetUpdateState,
    type LevelDatPresence,
    type ServerDatapackEntry,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import DatapackConceptHelp from '$lib/onboarding/DatapackConceptHelp.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import BulkActionBar, { type BulkBarAction } from '$lib/ui/BulkActionBar.svelte';
  import { bulkNameLines, reportBulk, runBulk } from '$lib/ui/bulk-run';
  import { Icon } from '$lib/ui/icons';
  import { createListSelection } from '$lib/ui/list-selection.svelte';
  import { refocusAfterRemoval } from '$lib/ui/refocus-after-removal';
  import SelectRowCheckbox from '$lib/ui/SelectRowCheckbox.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import CardShell from '$lib/ui/cards/CardShell.svelte';
  import CardMedia from '$lib/ui/cards/CardMedia.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import type { CardAccent } from '$lib/ui/cards/card-status';
  import Modal from '$lib/ui/Modal.svelte';
  import VanillaTweaksBuilder from '$lib/vanillatweaks/VanillaTweaksBuilder.svelte';
  import { installedVtPacks } from '$lib/vanillatweaks/vt-selection';
  import { ignoredHintKey } from '$lib/worlds/datapack-state';
  import {
    badgeOf,
    canBulkToggle,
    isUpdatable,
    rowKey,
    serverToggleBlockedKey,
    serverWorldBlockedKey,
  } from './datapack-rows';

  // Installed pane for a server world's datapacks (Task 11). Modeled on
  // ServerPluginsInstalled — same toolbar/error-line/ConfirmDialog shape — but
  // diverges where datapacks genuinely differ: rows key on filename (not
  // sha1 — see datapack-rows.ts), a ghost row has no toggle and is cleared,
  // not removed (its file is already gone; the game drops the id itself), a
  // folder pack has no update affordance, and update-all must apply serially
  // because every update rewrites level.dat. `disabled` is handed down by the host
  // (ServerAddonsTab) already resolved from the server's running state, so
  // this component holds no server-state lookup of its own.
  let {
    serverId,
    mcVersion,
    disabled = false,
    reloadToken = 0,
    levelDat = $bindable(null),
    emptyDropzone,
    onEmptyChange = () => {},
  }: {
    serverId: string;
    /** Needed by the Vanilla Tweaks builder, which publishes per MC family. */
    mcVersion: string;
    disabled?: boolean;
    reloadToken?: number;
    /**
     * The world's level.dat presence (D2); null until loaded, or could not
     * tell. Null is not a verdict: every writer re-checks presence before
     * writing. Bindable so the host gates its own add paths (the tab-level
     * drop zone, the catalog) on the one read this pane makes.
     */
    levelDat?: LevelDatPresence | null;
    /** The host's full drop area, rendered in the empty list (passed only while it shows). */
    emptyDropzone?: Snippet;
    /** Loaded and empty — the host hides its strip meanwhile (DESIGN.md §14). */
    onEmptyChange?: (empty: boolean) => void;
  } = $props();

  let rows = $state<ServerDatapackEntry[]>([]);
  const worldBlock = $derived(serverWorldBlockedKey(levelDat));
  let loading = $state(false);
  let loadError = $state<string | null>(null);
  let actionError = $state<string | null>(null);

  let vtOpen = $state(false);
  let vtBusy = $state(false);
  // Which Vanilla Tweaks packs this world already holds. Read from the sidecar
  // rows, so the builder keeps no selection of its own.
  const vtInstalled = $derived(
    installedVtPacks(
      rows.map((r) => ({
        source: r.record.source,
        project_id: r.record.project_id,
        version_id: r.record.version_id,
      })),
    ),
  );

  // Build, install into the world, then report honestly: a partly-failed
  // build must not read as a success.
  async function buildVt(selection: [string, string[]][]) {
    vtBusy = true;
    try {
      const res = await commands.vtInstallToServer(serverId, selection);
      if (res.status === 'error') {
        actionError = formatError(res.error);
        return;
      }
      const failed = res.data.outcomes.filter((o) => !o.installed).length;
      if (failed > 0) {
        actionError = $t('addons.datapacks.vt.someFailed', { count: failed });
      } else {
        actionError = null;
        vtOpen = false;
      }
      await load();
    } finally {
      vtBusy = false;
    }
  }

  // Per-pack update-check results, keyed by rowKey (exact filename) so
  // they line up with the row identity — NOT sha1, which two of the three row
  // provenances leave empty (see datapack-rows.ts).
  let updateChecks = $state(new Map<string, AssetUpdateState>());
  let checkingUpdates = $state(false);
  // Per-row in-flight guard: serverUpdateDatapackOne mutates level.dat with no
  // backend concurrent-same-pack guard.
  let updatingKeys = $state(new Set<string>());
  let updatingAll = $state(false);

  let pendingRemove = $state<ServerDatapackEntry | null>(null);
  let removing = $state(false);

  // The bulk bar's selection, keyed like every per-row map (rowKey), cleared on a server switch.
  const selection = createListSelection(
    () => rows.map(rowKey),
    () => serverId,
  );
  type BulkAction = 'enable' | 'disable' | 'update' | 'remove';
  let bulkAction = $state<BulkAction | null>(null);
  let pendingBulkRemove = $state<ServerDatapackEntry[] | null>(null);
  let listEl = $state<HTMLElement | null>(null);
  let emptyListEl = $state<HTMLElement | null>(null);

  // A different server's checks must never bleed across a switch.
  $effect(() => {
    void serverId;
    updateChecks = new Map();
  });

  let gen = 0;
  async function load(): Promise<void> {
    const my = ++gen;
    loading = true;
    loadError = null;
    const res = await commands.serverListDatapacks(serverId);
    if (my !== gen) return; // superseded by a newer serverId/reloadToken change
    if (res.status === 'ok') {
      rows = res.data.entries;
      levelDat = res.data.level_dat;
    } else {
      // Clear the rows too, as WorldDatapacks.reload does: rows kept from the
      // last good load with `levelDat` gone would lift the level.dat gates
      // (a never-started world's toggles would turn live), and an error and a
      // stale, still-interactive list must never render together.
      rows = [];
      levelDat = null;
      loadError = formatError(res.error);
    }
    loading = false;
  }

  $effect(() => {
    void reloadToken;
    void serverId;
    rows = [];
    levelDat = null;
    loadError = null;
    void load();
  });

  // Empty is reported, never assumed: a list still loading or one that could not be read is not
  // empty — the host keeps its strip until the list says it is empty.
  const listEmpty = $derived(!loading && rows.length === 0 && !loadError);
  $effect(() => {
    onEmptyChange(listEmpty);
    return () => onEmptyChange(false);
  });

  // A server run rewrites the world: a start restores level.dat from
  // level.dat_old — what the only-old note asks for — and every save rewrites
  // the pack lists. So the list is read again when the server stops, rather
  // than only after the user leaves the tab and comes back. `disabled` is the
  // host's running flag.
  let wasRunning = untrack(() => disabled);
  $effect(() => {
    const running = disabled;
    if (wasRunning && !running) void load();
    wasRunning = running;
  });

  const updatableCount = $derived(
    rows.filter((r) => isUpdatable(r) && updateChecks.get(rowKey(r))?.kind === 'update_available')
      .length,
  );

  const selectedRows = $derived(rows.filter((r) => selection.selected.has(rowKey(r))));
  const bulkToEnable = $derived(selectedRows.filter((r) => canBulkToggle(r, levelDat, true)));
  const bulkToDisable = $derived(selectedRows.filter((r) => canBulkToggle(r, levelDat, false)));
  const bulkToUpdate = $derived(
    selectedRows.flatMap((r) => {
      const st = updateChecks.get(rowKey(r));
      return isUpdatable(r) && st?.kind === 'update_available'
        ? [{ row: r, latest: st.latest }]
        : [];
    }),
  );
  const bulkGhosts = $derived(pendingBulkRemove?.filter((r) => !r.present).length ?? 0);
  const bulkBusy = $derived(bulkAction !== null || removing || updatingAll || checkingUpdates);
  // The pane's lasting gates first (a running server, a world with only level.dat_old), each
  // with its own text; otherwise an action is off when no selected row can take it.
  function bulkGate(action: BulkBarAction, none: boolean, noneReason: string): BulkBarAction {
    if (disabled)
      return { ...action, disabled: true, disabledReason: $t('servers.mods.stopToManage') };
    if (worldBlock !== null) return { ...action, disabled: true, disabledReason: $t(worldBlock) };
    return { ...action, disabled: none, disabledReason: noneReason };
  }
  const bulkActions = $derived<BulkBarAction[]>([
    bulkGate(
      { id: 'enable', label: $t('servers.datapacks.enable') },
      bulkToEnable.length === 0,
      $t('ui.bulk.noneApplicable'),
    ),
    bulkGate(
      { id: 'disable', label: $t('servers.datapacks.disable') },
      bulkToDisable.length === 0,
      $t('ui.bulk.noneApplicable'),
    ),
    bulkGate(
      { id: 'update', label: $t('addons.installed.update') },
      bulkToUpdate.length === 0,
      $t('ui.bulk.updateNeedsCheck'),
    ),
    bulkGate(
      { id: 'remove', label: $t('servers.datapacks.remove'), intent: 'danger' },
      selectedRows.length === 0,
      $t('ui.bulk.noneApplicable'),
    ),
  ]);
  const bulkAllowed = $derived(!disabled && worldBlock === null);

  function rowAccent(entry: ServerDatapackEntry, key: string): CardAccent {
    if (entry.state === 'ignored') return 'warning';
    const state = updateChecks.get(key);
    if (state?.kind === 'update_available' || state?.kind === 'check_failed') return 'warning';
    return 'none';
  }

  // Read-only scan: classify every identity-bearing pack against its platform.
  async function checkUpdates() {
    checkingUpdates = true;
    actionError = null;
    try {
      const res = await commands.serverCheckDatapackUpdates(serverId);
      if (res.status === 'ok') {
        const m = new Map<string, AssetUpdateState>();
        for (const c of res.data) m.set(c.filename, c.state);
        updateChecks = m;
      } else actionError = formatError(res.error);
    } finally {
      checkingUpdates = false;
    }
  }

  // Apply one pending update. `update_one` checks the old file's identity
  // before it places the new one, so a foreign old file is refused outright
  // and an `ok` outcome always carries `completed: true` — there is no partial
  // state left to render. If that ever changes, the UI has to learn the
  // partial case again.
  async function updateOne(row: ServerDatapackEntry) {
    const key = rowKey(row);
    const state = updateChecks.get(key);
    if (!state || state.kind !== 'update_available') return;
    if (disabled) {
      actionError = $t('servers.mods.stopToManage');
      return;
    }
    if (updatingKeys.has(key)) return;
    updatingKeys = new Set(updatingKeys).add(key);
    actionError = null;
    try {
      const res = await commands.serverUpdateDatapackOne(
        serverId,
        row.record.filename,
        state.latest,
      );
      if (res.status === 'ok') {
        const next = new Map(updateChecks);
        next.delete(key);
        updateChecks = next;
        await load();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      const s = new Set(updatingKeys);
      s.delete(key);
      updatingKeys = s;
    }
  }

  // Apply every pending update in one sequential batch — each update rewrites
  // level.dat, so applies MUST stay serial, never interleaved. A hard failure
  // (res.status === 'error') stops the batch, leaving the remaining rows their
  // update affordance untouched.
  async function updateAll() {
    if (disabled) {
      actionError = $t('servers.mods.stopToManage');
      return;
    }
    const targets = rows
      .filter((r) => isUpdatable(r))
      .map((r) => ({ row: r, state: updateChecks.get(rowKey(r)) }))
      .filter(
        (
          x,
        ): x is {
          row: ServerDatapackEntry;
          state: Extract<AssetUpdateState, { kind: 'update_available' }>;
        } => x.state?.kind === 'update_available',
      );
    if (targets.length === 0) return;
    actionError = null;
    updatingAll = true;
    try {
      for (const { row, state } of targets) {
        const key = rowKey(row);
        const res = await commands.serverUpdateDatapackOne(
          serverId,
          row.record.filename,
          state.latest,
        );
        if (res.status === 'error') {
          actionError = formatError(res.error);
          break;
        }
        const next = new Map(updateChecks);
        next.delete(key);
        updateChecks = next;
      }
      await load();
    } finally {
      updatingAll = false;
    }
  }

  // Enable/disable in level.dat. A ghost row (present === false) never renders
  // this control — there is no file to enable — and neither does a row the
  // game ignores: switching it on would change nothing it loads.
  async function toggle(row: ServerDatapackEntry) {
    if (disabled) {
      actionError = $t('servers.mods.stopToManage');
      return;
    }
    actionError = null;
    const res = await commands.serverSetDatapackEnabled(
      serverId,
      row.record.filename,
      row.state !== 'enabled',
    );
    if (res.status === 'ok') await load();
    else actionError = formatError(res.error);
  }

  async function confirmDelete(row: ServerDatapackEntry) {
    actionError = null;
    removing = true;
    try {
      const res = await commands.serverRemoveDatapack(serverId, row.record.filename);
      if (res.status === 'ok') {
        pendingRemove = null;
        const key = rowKey(row);
        if (updateChecks.has(key)) {
          const next = new Map(updateChecks);
          next.delete(key);
          updateChecks = next;
        }
        await load();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      removing = false;
    }
  }

  function onBulkAction(id: string): void {
    if (id === 'enable') void bulkSetEnabled(true);
    else if (id === 'disable') void bulkSetEnabled(false);
    else if (id === 'update') void bulkUpdate();
    else if (id === 'remove') requestBulkRemove();
  }

  async function bulkSetEnabled(enable: boolean): Promise<void> {
    const id = serverId;
    const targets = enable ? bulkToEnable : bulkToDisable;
    if (!bulkAllowed || targets.length === 0) return;
    bulkAction = enable ? 'enable' : 'disable';
    actionError = null;
    try {
      const outcome = await runBulk(
        targets,
        (r) => commands.serverSetDatapackEnabled(id, r.record.filename, enable),
        formatError,
        (r) => r.record.name ?? r.record.filename,
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
    await load();
  }

  // Serial, as every update rewrites level.dat; unlike «Update all», a failure is counted and
  // named rather than stopping the run.
  async function bulkUpdate(): Promise<void> {
    const id = serverId;
    const targets = bulkToUpdate;
    if (!bulkAllowed || targets.length === 0) return;
    bulkAction = 'update';
    actionError = null;
    try {
      const outcome = await runBulk(
        targets,
        (tg) => commands.serverUpdateDatapackOne(id, tg.row.record.filename, tg.latest),
        formatError,
        (tg) => tg.row.record.name ?? tg.row.record.filename,
        (tg) => {
          const next = new Map(updateChecks);
          next.delete(rowKey(tg.row));
          updateChecks = next;
        },
      );
      reportBulk(outcome, { done: 'ui.bulk.updated', partial: 'ui.bulk.updatedFailed' });
    } finally {
      bulkAction = null;
    }
    selection.clear();
    await load();
  }

  function requestBulkRemove(): void {
    if (!bulkAllowed || selectedRows.length === 0) return;
    actionError = null;
    pendingBulkRemove = selectedRows;
  }

  async function confirmBulkRemove(): Promise<void> {
    const id = serverId;
    const targets = pendingBulkRemove;
    if (!targets) return;
    const index = Math.max(
      0,
      rows.findIndex((r) => selection.selected.has(rowKey(r))),
    );
    bulkAction = 'remove';
    try {
      const outcome = await runBulk(
        targets,
        (r) => commands.serverRemoveDatapack(id, r.record.filename),
        formatError,
        (r) => r.record.name ?? r.record.filename,
        (r) => {
          const key = rowKey(r);
          if (updateChecks.has(key)) {
            const next = new Map(updateChecks);
            next.delete(key);
            updateChecks = next;
          }
        },
      );
      pendingBulkRemove = null;
      reportBulk(outcome, { done: 'ui.bulk.removed', partial: 'ui.bulk.removedFailed' });
    } finally {
      bulkAction = null;
    }
    selection.clear();
    await load();
    await refocusAfterRemoval({
      index,
      listEl,
      rows: (list) => [
        ...list.querySelectorAll<HTMLElement>('[data-testid="server-datapack-row"]'),
      ],
      emptyEl: emptyListEl,
    });
  }

  onDestroy(() => selection.dispose());
</script>

<div class="flex flex-col gap-3" data-testid="server-datapacks-installed">
  <div class="flex flex-wrap items-center gap-2">
    <DatapackConceptHelp />
    <BusyButton
      class="btn-secondary btn-sm"
      data-testid="server-datapacks-check-updates"
      busy={checkingUpdates}
      {disabled}
      onclick={() => void checkUpdates()}
    >
      {$t('servers.datapacks.checkUpdates')}
    </BusyButton>
    <BusyButton
      class="btn-warning btn-sm"
      data-testid="server-datapacks-update-all"
      busy={updatingAll}
      disabled={disabled || worldBlock !== null || updatableCount === 0}
      onclick={() => void updateAll()}
    >
      {$t('mods.installed.updateAll', { count: updatableCount })}
    </BusyButton>
    <button
      type="button"
      class="btn-secondary btn-sm"
      data-testid="server-open-vt-builder"
      disabled={disabled || worldBlock !== null}
      onclick={() => (vtOpen = true)}
    >
      {$t('addons.datapacks.vt.open')}
    </button>
  </div>

  {#if vtOpen}
    <Modal
      onClose={() => (vtOpen = false)}
      ariaLabel={$t('addons.datapacks.vt.title')}
      panelClass="max-w-2xl w-full"
      dataTestid="server-vt-builder-modal"
    >
      <VanillaTweaksBuilder {mcVersion} installed={vtInstalled} busy={vtBusy} onBuild={buildVt} />
    </Modal>
  {/if}

  {#if disabled}
    <p class="text-xs text-warning-text">{$t('servers.mods.stopToManage')}</p>
  {/if}

  <p class="text-xs text-secondary">{$t('servers.datapacks.note')}</p>

  {#if levelDat === 'absent'}
    <p class="text-xs text-secondary" data-testid="server-datapacks-level-dat-note">
      {$t('servers.datapacks.notGenerated')}
    </p>
  {:else if levelDat === 'only_old'}
    <p class="text-xs text-warning-text" data-testid="server-datapacks-level-dat-note">
      {$t('servers.datapacks.onlyOldLevelDat')}
    </p>
  {/if}

  {#if loadError}
    <p class="text-sm text-danger" role="alert">{loadError}</p>
  {/if}
  {#if actionError}
    <p class="text-sm text-danger" role="alert">{actionError}</p>
  {/if}

  {#if loading && rows.length === 0}
    <LoadingPanel label={$t('mods.installed.loading')} />
  {:else if rows.length === 0 && !loadError}
    <!-- The host's full drop area replaces its strip here (DESIGN.md §14). Focus lands here after
         the last removal (`refocusAfterRemoval`): a parking place that reads the message, not a
         control. -->
    <div
      bind:this={emptyListEl}
      tabindex="-1"
      class="flex flex-col gap-3 outline-none"
      data-testid="list-empty"
    >
      <p class="text-sm text-muted">{$t('servers.datapacks.empty')}</p>
      {@render emptyDropzone?.()}
    </div>
  {:else if rows.length > 0}
    <div bind:this={listEl} class="border border-border-subtle rounded-lg overflow-hidden">
      <BulkActionBar
        allSelected={selection.allSelected}
        indeterminate={selection.indeterminate}
        selectedCount={selection.count}
        busy={bulkBusy}
        busyAction={bulkAction}
        hint={$t('servers.datapacks.bulkHint')}
        actions={bulkActions}
        onToggleAll={selection.toggleAll}
        onAction={onBulkAction}
        onClear={selection.clear}
      />
      {#each rows as row (rowKey(row))}
        {@const key = rowKey(row)}
        {@const badge = badgeOf(row)}
        {@const check = updateChecks.get(key)}
        {@const canUpdateRow = isUpdatable(row) && check?.kind === 'update_available'}
        {@const rowBusy = updatingKeys.has(key)}
        <CardShell variant="row" accent={rowAccent(row, key)} testid="server-datapack-row">
          <SelectRowCheckbox
            checked={selection.selected.has(key)}
            name={row.record.name ?? row.record.filename}
            onChange={(c) => selection.toggle(key, c)}
          />
          <CardMedia placeholder={row.is_folder ? 'folderOpen' : 'datapack'} size="sm" />
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-center gap-2">
              <span class="truncate text-sm font-medium text-primary">
                {row.record.name ?? row.record.filename}
              </span>
              <StatusBadge variant={badge.variant}>{$t(badge.labelKey)}</StatusBadge>
              {#if check && check.kind === 'check_failed'}
                <span
                  class="shrink-0 text-warning-text"
                  role="img"
                  aria-label={$t('addons.installed.checkFailed')}
                  use:tooltip={{ text: check.reason, describe: false }}
                >
                  <Icon name="warning" size={13} />
                </span>
              {/if}
            </div>
            <div class="truncate text-xs text-muted">
              <span class="font-mono">{row.record.filename}</span>
              {#if row.record.version_number}
                <span class="ml-2">{row.record.version_number}</span>
              {/if}
            </div>
            {#if row.state === 'ignored'}
              {@const hint = ignoredHintKey(row.ignored_reason)}
              {#if hint}<p
                  class="text-xs text-warning-text"
                  data-testid="server-datapack-ignored-hint"
                >
                  {$t(hint)}
                </p>{/if}
            {/if}
          </div>

          <!-- Each tooltip rides a wrapping span, so a control disabled by the
               world's level.dat (D2) still explains itself, keyboard included —
               the WorldDatapacks shape. -->
          {#if canUpdateRow}
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <span
              class="inline-flex"
              tabindex={worldBlock !== null ? 0 : undefined}
              use:tooltip={{
                text: worldBlock !== null ? $t(worldBlock) : $t('addons.installed.update'),
                describe: false,
              }}
            >
              <button
                type="button"
                class="btn-icon btn-icon-sm btn-icon-warning"
                data-testid="server-datapack-update"
                disabled={disabled ||
                  worldBlock !== null ||
                  checkingUpdates ||
                  updatingAll ||
                  rowBusy}
                onclick={() => void updateOne(row)}
                aria-label={$t('addons.installed.update')}
              >
                <Icon name="refresh" size={15} />
              </button>
            </span>
          {/if}

          {#if row.present && row.state !== 'ignored'}
            {@const toggleBlock = serverToggleBlockedKey(levelDat, row.state)}
            {@const toggleLabel =
              row.state === 'enabled'
                ? $t('servers.datapacks.disable')
                : $t('servers.datapacks.enable')}
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <span
              class="inline-flex"
              tabindex={toggleBlock !== null ? 0 : undefined}
              use:tooltip={{
                text: toggleBlock !== null ? $t(toggleBlock) : toggleLabel,
                describe: false,
              }}
            >
              <button
                type="button"
                class={`btn-icon btn-icon-sm ${row.state === 'enabled' ? 'btn-icon-success' : '!text-muted'}`}
                data-testid="server-datapack-toggle"
                disabled={disabled || toggleBlock !== null}
                onclick={() => void toggle(row)}
                aria-label={toggleLabel}
              >
                <Icon name="power" size={15} />
              </button>
            </span>
          {/if}

          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <span
            class="inline-flex"
            tabindex={worldBlock !== null ? 0 : undefined}
            use:tooltip={{
              text: worldBlock !== null ? $t(worldBlock) : $t('servers.datapacks.remove'),
              describe: false,
            }}
          >
            <button
              type="button"
              class="btn-icon btn-icon-sm btn-icon-danger"
              data-testid="server-datapack-remove"
              disabled={disabled || worldBlock !== null}
              onclick={() => {
                actionError = null;
                pendingRemove = row;
              }}
              aria-label={$t('servers.datapacks.remove')}
            >
              <Icon name="trash" size={15} />
            </button>
          </span>
        </CardShell>
      {/each}
    </div>
  {/if}

  {#if pendingRemove}
    <ConfirmDialog
      title={pendingRemove.present
        ? $t('servers.datapacks.remove')
        : $t('worlds.datapacks.clearEntry')}
      bodyText={pendingRemove.present
        ? $t('servers.datapacks.removeConfirm', {
            name: pendingRemove.record.name ?? pendingRemove.record.filename,
          })
        : $t('servers.datapacks.removeGhostConfirm', {
            name: pendingRemove.record.name ?? pendingRemove.record.filename,
          })}
      confirmLabel={pendingRemove.present
        ? $t('servers.datapacks.remove')
        : $t('worlds.datapacks.clearEntry')}
      variant={pendingRemove.present ? 'danger' : 'primary'}
      busy={removing}
      error={actionError}
      onCancel={() => (pendingRemove = null)}
      onConfirm={() => pendingRemove && void confirmDelete(pendingRemove)}
    />
  {/if}

  {#if pendingBulkRemove}
    <ConfirmDialog
      title={$t('servers.datapacks.removeManyConfirm', { count: pendingBulkRemove.length })}
      bodyText={[
        ...bulkNameLines(pendingBulkRemove.map((r) => r.record.name ?? r.record.filename)),
        ...(bulkGhosts > 0
          ? [$t('servers.datapacks.removeManyGhostNote', { count: bulkGhosts })]
          : []),
      ]}
      confirmLabel={$t('servers.datapacks.remove')}
      variant="danger"
      busy={bulkAction === 'remove'}
      confirmTestid="server-datapacks-bulk-remove-confirm"
      onCancel={() => (pendingBulkRemove = null)}
      onConfirm={() => void confirmBulkRemove()}
    />
  {/if}
</div>
