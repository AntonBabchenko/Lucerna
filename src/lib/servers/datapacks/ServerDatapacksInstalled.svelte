<script lang="ts">
  import { commands, type AssetUpdateState, type ServerDatapackEntry } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { Icon } from '$lib/ui/icons';
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
  import { badgeOf, isUpdatable, rowKey } from './datapack-rows';

  // Installed pane for a server world's datapacks (Task 11). Modeled on
  // ServerPluginsInstalled — same toolbar/error-line/ConfirmDialog shape — but
  // diverges where datapacks genuinely differ: rows key on filename (not
  // sha1 — see datapack-rows.ts), a ghost row has no toggle, a folder pack has
  // no update affordance, and update-all must apply serially because every
  // update rewrites level.dat. `disabled` is handed down by the host
  // (ServerAddonsTab) already resolved from the server's running state, so
  // this component holds no server-state lookup of its own.
  let {
    serverId,
    mcVersion,
    disabled = false,
    reloadToken = 0,
  }: {
    serverId: string;
    /** Needed by the Vanilla Tweaks builder, which publishes per MC family. */
    mcVersion: string;
    disabled?: boolean;
    reloadToken?: number;
  } = $props();

  let rows = $state<ServerDatapackEntry[]>([]);
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

  // Per-pack update-check results, keyed by rowKey (lower-cased filename) so
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
    if (res.status === 'ok') rows = res.data;
    else loadError = formatError(res.error);
    loading = false;
  }

  $effect(() => {
    void reloadToken;
    void serverId;
    rows = [];
    loadError = null;
    void load();
  });

  const updatableCount = $derived(
    rows.filter((r) => isUpdatable(r) && updateChecks.get(rowKey(r))?.kind === 'update_available')
      .length,
  );

  function rowAccent(entry: ServerDatapackEntry, key: string): CardAccent {
    if (!entry.present) return 'danger';
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
        for (const c of res.data) m.set(c.filename.toLowerCase(), c.state);
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
  // this control — there is no file to enable.
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
</script>

<div class="flex flex-col gap-3" data-testid="server-datapacks-installed">
  <div class="flex flex-wrap items-center gap-2">
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
      disabled={disabled || updatableCount === 0}
      onclick={() => void updateAll()}
    >
      {$t('mods.installed.updateAll', { count: updatableCount })}
    </BusyButton>
    <button
      type="button"
      class="btn-secondary btn-sm"
      data-testid="server-open-vt-builder"
      {disabled}
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

  {#if loadError}
    <p class="text-sm text-danger" role="alert">{loadError}</p>
  {/if}
  {#if actionError}
    <p class="text-sm text-danger" role="alert">{actionError}</p>
  {/if}

  {#if loading && rows.length === 0}
    <LoadingPanel label={$t('mods.installed.loading')} />
  {:else if rows.length === 0 && !loadError}
    <p class="text-sm text-muted">{$t('servers.datapacks.empty')}</p>
  {:else if rows.length > 0}
    <div class="border border-border-subtle rounded-lg overflow-hidden">
      {#each rows as row (rowKey(row))}
        {@const key = rowKey(row)}
        {@const badge = badgeOf(row)}
        {@const check = updateChecks.get(key)}
        {@const canUpdateRow = isUpdatable(row) && check?.kind === 'update_available'}
        {@const rowBusy = updatingKeys.has(key)}
        <CardShell variant="row" accent={rowAccent(row, key)}>
          <CardMedia placeholder={row.is_folder ? 'folderOpen' : 'world'} size="sm" />
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
          </div>

          {#if canUpdateRow}
            <button
              type="button"
              class="btn-icon btn-icon-sm btn-icon-warning"
              disabled={disabled || checkingUpdates || updatingAll || rowBusy}
              onclick={() => void updateOne(row)}
              aria-label={$t('addons.installed.update')}
              use:tooltip={$t('addons.installed.update')}
            >
              <Icon name="refresh" size={15} />
            </button>
          {/if}

          {#if row.present}
            <button
              type="button"
              class={`btn-icon btn-icon-sm ${row.state === 'enabled' ? 'btn-icon-success' : '!text-muted'}`}
              {disabled}
              onclick={() => void toggle(row)}
              aria-label={row.state === 'enabled'
                ? $t('servers.datapacks.disable')
                : $t('servers.datapacks.enable')}
              use:tooltip={row.state === 'enabled'
                ? $t('servers.datapacks.disable')
                : $t('servers.datapacks.enable')}
            >
              <Icon name="power" size={15} />
            </button>
          {/if}

          <button
            type="button"
            class="btn-icon btn-icon-sm btn-icon-danger"
            {disabled}
            onclick={() => {
              actionError = null;
              pendingRemove = row;
            }}
            aria-label={$t('servers.datapacks.remove')}
            use:tooltip={$t('servers.datapacks.remove')}
          >
            <Icon name="trash" size={15} />
          </button>
        </CardShell>
      {/each}
    </div>
  {/if}

  {#if pendingRemove}
    <ConfirmDialog
      title={$t('servers.datapacks.remove')}
      bodyText={pendingRemove.present
        ? $t('servers.datapacks.removeConfirm', {
            name: pendingRemove.record.name ?? pendingRemove.record.filename,
          })
        : $t('servers.datapacks.removeGhostConfirm', {
            name: pendingRemove.record.name ?? pendingRemove.record.filename,
          })}
      confirmLabel={$t('servers.datapacks.remove')}
      variant="danger"
      busy={removing}
      error={actionError}
      onCancel={() => (pendingRemove = null)}
      onConfirm={() => pendingRemove && void confirmDelete(pendingRemove)}
    />
  {/if}
</div>
