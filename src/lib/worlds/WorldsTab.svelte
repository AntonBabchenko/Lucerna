<script lang="ts">
  import { commands, events, type World } from '$lib/ipc/bindings';
  import type { InstanceWithStatus, MigrationMode, MigrationOutcome } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { listenUntilDestroyed } from '$lib/ipc/listen';
  import { relativeTime } from '$lib/format/relative-time';
  import { formatSize } from '$lib/format/size';
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import { WORLDS_STEPS } from '$lib/onboarding/contextual-tours';
  import WorldDetailDialog from '$lib/worlds/WorldDetailDialog.svelte';
  import MigrateWorldDialog from '$lib/worlds/MigrateWorldDialog.svelte';
  import { buildMigrationToast } from '$lib/worlds/migrate-toast';
  import OrphanedSection from '$lib/worlds/OrphanedSection.svelte';
  import type { OrphanedBackupSet, StrandedWorld } from '$lib/ipc/bindings';
  import DeleteWorldDialog from '$lib/worlds/DeleteWorldDialog.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { t } from '$lib/i18n';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import FileDropzone from '$lib/mods/FileDropzone.svelte';
  import { droppedWorld } from '$lib/settings/state.svelte';
  import { dataLocation } from '$lib/settings/data-location.svelte';
  import { dataRootCreateDisabledKey } from '$lib/settings/data-root-gating';

  let {
    instanceId,
    instanceName = null,
    instances = [],
    onListChanged,
    onWorldsChanged = () => {},
    onQuickPlayWorld = () => {},
    quickPlayDisabledReason = null,
    running = false,
  }: {
    instanceId: string | null;
    /** Display name of `instanceId` (the page's `activeInstance.name`) — the
     *  migrate dialog and its toast name the source by it, never by id. Null
     *  only while `instanceId` is null, when no dialog can open. */
    instanceName?: string | null;
    /** Every instance, for the migrate dialog's target picker. The page owns
     *  the list; it is passed down rather than fetched a second time here. */
    instances?: InstanceWithStatus[];
    onListChanged: () => void;
    /** This tab's world set changed on disk (a migration landed). The parent
     *  refreshes whatever else lists worlds — the sidebar's Play menu — which
     *  nothing in this tab can reach. */
    onWorldsChanged?: () => void;
    onQuickPlayWorld?: (folderName: string) => void;
    quickPlayDisabledReason?: string | null;
    running?: boolean;
  } = $props();

  let worlds = $state<World[]>([]);
  let listError = $state<string | null>(null);
  let loading = $state(false);
  let detailFor = $state<World | null>(null);
  let deleteFor = $state<World | null>(null);
  // The world a MigrateWorldDialog is open for; set from the detail dialog's
  // footer action, cleared on close or completion.
  let migrateFor = $state<World | null>(null);
  // What a failed restore can leave behind. Both are invisible to listWorlds —
  // their names start with a dot, which validate_segment rejects — so they need
  // their own queries. Failures here are non-fatal: they leave the recovery
  // section hidden rather than breaking the world list.
  let orphans = $state<OrphanedBackupSet[]>([]);
  let stranded = $state<StrandedWorld[]>([]);

  const sourceName = $derived(instanceName ?? '');

  async function reloadRecoverable(reqId: string) {
    const [o, s] = await Promise.all([
      commands.listOrphanedBackupWorlds(reqId),
      commands.listStrandedWorlds(reqId),
    ]);
    if (instanceId !== reqId) return;
    orphans = o.status === 'ok' ? o.data : [];
    stranded = s.status === 'ok' ? s.data : [];
  }

  async function reload() {
    if (!instanceId) {
      worlds = [];
      orphans = [];
      stranded = [];
      return;
    }
    // Capture the instance this load is for; a rapid instance switch mid-fetch
    // must not commit the previous instance's worlds over the newer selection.
    const reqId = instanceId;
    loading = true;
    listError = null;
    // Clear here, not only in the no-instance branch: otherwise instance A's
    // "Interrupted restore" row stays on screen for the whole of instance B's
    // load, and a click in that window sends A's directory name with B's id.
    orphans = [];
    stranded = [];
    const r = await commands.listWorlds(reqId);
    if (instanceId !== reqId) return;
    loading = false;
    if (r.status === 'ok') {
      worlds = r.data;
    } else {
      listError = formatError(r.error);
    }
    await reloadRecoverable(reqId);
  }

  $effect(() => {
    void instanceId;
    void reload();
  });

  // Refresh after MC exits — size + mtime + backup_count can change
  // (a backup taken pre-launch, a new region file written by the game).
  // Race-safe subscribe: the old late-assigned-unlisten effect leaked the
  // listener when the tab unmounted before listen() resolved.
  listenUntilDestroyed([events.processExited.listen(() => void reload())]);

  async function onBackupNow(w: World) {
    if (!instanceId) return;
    const r = await commands.backupWorld(instanceId, w.folder_name);
    if (r.status === 'ok') {
      pushSuccess(
        $t('worlds.tab.toastBackedUp', {
          name: w.folder_name,
          size: formatSize($t, r.data.size_bytes),
        }),
      );
      await reload();
    } else {
      pushWarning(formatError(r.error));
    }
  }

  async function onOpenSavesFolder() {
    if (!instanceId) return;
    const r = await commands.openSavesFolder(instanceId);
    if (r.status !== 'ok') pushWarning(formatError(r.error));
  }

  // §7 fallback gating: world import writes into the instance's saves dir,
  // which would land in the wrong (temporary default) root while the
  // configured data root is unavailable. See data-root-gating.ts.
  const importDisabledReason = $derived.by(() => {
    const key = dataRootCreateDisabledKey(dataLocation.fellBack);
    return key === null ? null : $t(key);
  });

  // Entry-point gating for the migrate action (world-migration spec §7). The
  // same data-root key as import — a migration writes into ANOTHER instance's
  // saves, which would land in the temporary root while fallen back — and
  // "stop the source first" while it is running. The backend refuses both
  // regardless (reject_if_fallen_back / WorldMigrateInstanceRunning, which
  // also covers a merely STARTING source this tab does not know about); this
  // only keeps the button honest about why it will not work.
  const migrateDisabledReason = $derived.by(() => {
    if (importDisabledReason !== null) return importDisabledReason;
    if (running) return $t('worlds.migrate.entry.disabledRunning', { name: sourceName });
    return null;
  });

  // After MigrateWorldDialog reports a landed migration (its `onDone` fires
  // only for `status: 'ok'`). Order: toast first — the user sees the outcome
  // the moment it exists — then this tab's list, then the parent's Play menu.
  async function onMigrateDone(
    world: World,
    r: { mode: MigrationMode; outcome: MigrationOutcome; targetName: string },
  ) {
    migrateFor = null;
    // A moved world no longer exists in this instance: a detail dialog left
    // open on it would show backups and datapacks of a folder that is gone.
    // A copied world is still here, so its dialog stays.
    if (r.mode === 'move' && detailFor?.folder_name === world.folder_name) detailFor = null;
    const toast = buildMigrationToast($t, {
      mode: r.mode,
      outcome: r.outcome,
      sourceWorld: world.folder_name,
      sourceName,
      targetName: r.targetName,
    });
    if (toast.kind === 'warning') pushWarning(toast.title, toast.lines);
    else pushSuccess(toast.title, toast.lines);
    await reload();
    onWorldsChanged();
  }

  // One core for all entry points (dropzone click, folder button, and the
  // drag-drop consume effect). Imports each path; the backend decides zip vs
  // folder and returns a typed error per path.
  async function importPaths(paths: string[]) {
    if (!instanceId || paths.length === 0) return;
    // Belt-and-braces: entry points are also disabled via importDisabledReason.
    if (dataLocation.fellBack) return;
    let added = 0;
    for (const p of paths) {
      const r = await commands.worldImport(instanceId, p);
      if (r.status === 'ok') {
        pushSuccess($t('worlds.import.toastAdded', { name: r.data.folder_name }));
        added++;
      } else {
        pushWarning(formatError(r.error));
      }
    }
    if (added > 0) {
      onListChanged();
      await reload();
    }
  }

  async function onImport(source: 'zip' | 'folder') {
    if (!instanceId) return;
    if (dataLocation.fellBack) return;
    const picked =
      source === 'zip'
        ? await openFile({
            multiple: false,
            filters: [{ name: $t('common.fileFilter.worldZip'), extensions: ['zip'] }],
          })
        : await openFile({ directory: true });
    if (typeof picked === 'string') await importPaths([picked]);
  }

  // Paths dropped on the Worlds tab arrive via droppedWorld (routed by
  // MainTabs). Consume and reset; the backend validates each path.
  $effect(() => {
    const v = droppedWorld.value;
    if (v !== null) {
      droppedWorld.value = null;
      if (dataLocation.fellBack) return;
      void importPaths(v);
    }
  });
</script>

<div class="p-3 flex flex-col gap-2" data-testid="worlds-tab">
  <div data-tour-ctx="worlds-import">
    <FileDropzone
      label={$t('worlds.import.dropzoneLabel')}
      disabled={!instanceId || importDisabledReason !== null}
      disabledLabel={importDisabledReason ?? undefined}
      onClick={() => void onImport('zip')}
    />
  </div>
  <div class="flex flex-wrap items-center gap-2">
    <span class="inline-flex" use:tooltip={{ text: importDisabledReason ?? '', describe: false }}>
      <button
        type="button"
        class="btn-tertiary inline-flex items-center gap-1"
        disabled={!instanceId || importDisabledReason !== null}
        onclick={() => void onImport('folder')}
      >
        <Icon name="folderOpen" size={14} />
        {$t('worlds.import.fromFolder')}
      </button>
    </span>
    <button
      type="button"
      class="btn-tertiary inline-flex items-center gap-1"
      data-tour-ctx="worlds-open-folder"
      onclick={() => void onOpenSavesFolder()}
    >
      {$t('worlds.tab.openSavesFolder')}
      <Icon name="folderOpen" size={14} />
    </button>
  </div>
  {#if !instanceId}
    <p class="text-sm text-muted">{$t('worlds.tab.noInstance')}</p>
  {:else if loading}
    <LoadingPanel label={$t('worlds.tab.loading')} />
  {:else if listError}
    <p class="text-sm text-danger">{listError}</p>
  {:else if worlds.length === 0 && orphans.length === 0 && stranded.length === 0}
    <p class="text-sm text-muted">{$t('worlds.tab.empty')}</p>
  {:else if worlds.length === 0}
    <!-- Nothing playable, but something recoverable: render no list and no
         empty-state copy. "Play Minecraft to create one" directly above
         "Interrupted restore" would be an odd thing to read when the user's
         world is sitting one click away. The recovery section below the chain
         carries this case. -->
  {:else}
    <ul
      class="border border-border-subtle rounded divide-y divide-border-subtle"
      data-tour-ctx="worlds-list"
    >
      {#each worlds as w (w.folder_name)}
        <!-- Deliberate: the whole row is the primary affordance (opens the
             world-detail dialog — Backups | Datapacks), with keyboard
             activation below — see spec §9. -->
        <!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
        <li
          class="flex items-center justify-between gap-2 px-3 py-2 hover:bg-subtle cursor-pointer"
          data-testid="world-row"
          role="button"
          tabindex="0"
          aria-label={$t('worlds.tab.openBackupsAriaLabel', { name: w.folder_name })}
          onclick={() => (detailFor = w)}
          onkeydown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault();
              detailFor = w;
            }
          }}
        >
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span
                class="font-medium truncate"
                use:tooltip={{ text: w.folder_name, whenOverflowing: true }}>{w.folder_name}</span
              >
              {#if w.backup_count > 0}
                <span
                  class="inline-flex items-center gap-1 text-xs text-warning-text bg-warning-bg rounded px-1.5 py-0.5"
                  aria-label={$t('worlds.tab.backupCountAriaLabel', { count: w.backup_count })}
                  use:tooltip={$t('worlds.tab.backupCountAriaLabel', { count: w.backup_count })}
                >
                  <Icon name="package" size={12} />
                  {w.backup_count}
                </span>
              {/if}
            </div>
            <div class="text-xs text-muted">
              {formatSize($t, w.size_bytes)} · {relativeTime($t, w.modified_unix_ms)}
            </div>
          </div>
          <div class="flex items-center gap-1 flex-shrink-0">
            <span
              class="inline-flex"
              use:tooltip={{
                text: quickPlayDisabledReason ?? $t('worlds.quickPlay.playWorld'),
                describe: false,
              }}
            >
              <button
                type="button"
                class="btn-success btn-sm px-2 disabled:opacity-40 disabled:cursor-not-allowed"
                disabled={quickPlayDisabledReason !== null}
                aria-label={$t('worlds.quickPlay.playWorld')}
                onclick={(e) => {
                  e.stopPropagation();
                  onQuickPlayWorld(w.folder_name);
                }}
              >
                <Icon name="play" size={16} />
              </button>
            </span>
            <button
              type="button"
              class="btn-icon btn-icon-sm"
              data-testid="world-backup-btn"
              aria-label={$t('worlds.tab.backupNow')}
              use:tooltip={$t('worlds.tab.backupNow')}
              onclick={(e) => {
                e.stopPropagation();
                void onBackupNow(w);
              }}
            >
              <Icon name="archive" size={15} />
            </button>
            <button
              type="button"
              class="btn-icon btn-icon-sm btn-icon-danger"
              data-testid="world-delete-btn"
              aria-label={$t('worlds.tab.deleteWorld')}
              use:tooltip={$t('worlds.tab.deleteWorld')}
              onclick={(e) => {
                e.stopPropagation();
                deleteFor = w;
              }}
            >
              <Icon name="trash" size={15} />
            </button>
            <span class="text-placeholder flex-shrink-0" aria-hidden="true">
              <Icon name="chevronRight" size={16} />
            </span>
          </div>
        </li>
      {/each}
    </ul>
  {/if}

  <!-- AFTER the exclusive chain above, on purpose. Placed inside its {:else}
       this would never render when worlds.length === 0 — i.e. on the instance
       whose only world was the one that got stranded, the exact case it exists
       for. -->
  {#if instanceId}
    <OrphanedSection
      {instanceId}
      {orphans}
      {stranded}
      onChanged={() => {
        void reload();
        onListChanged();
      }}
    />
  {/if}

  <!-- Tour fires only once worlds exist — most steps point at the
       list which is absent on a fresh instance. -->
  {#if worlds.length > 0}
    <ContextualTour id="worlds" steps={WORLDS_STEPS} />
  {/if}
</div>

{#if detailFor && instanceId}
  <WorldDetailDialog
    {instanceId}
    world={detailFor}
    {running}
    {migrateDisabledReason}
    onMigrate={() => (migrateFor = detailFor)}
    onClose={() => {
      detailFor = null;
      void reload();
    }}
    onChanged={() => {
      onListChanged();
      void reload();
    }}
  />
{/if}

<!-- Rendered AFTER WorldDetailDialog on purpose: Modals share z-50 and stack
     by DOM order (DESIGN.md §8), and this one opens on top of the detail
     dialog it was summoned from. The {@const} pins the world for the callback:
     the {#if} narrows `migrateFor` for direct reads, not inside closures. -->
{#if migrateFor && instanceId}
  {@const world = migrateFor}
  <MigrateWorldDialog
    {instanceId}
    instanceName={sourceName}
    {world}
    {instances}
    onClose={() => (migrateFor = null)}
    onDone={(r) => void onMigrateDone(world, r)}
  />
{/if}

{#if deleteFor && instanceId}
  <DeleteWorldDialog
    {instanceId}
    world={deleteFor}
    onClose={() => (deleteFor = null)}
    onDeleted={() => {
      deleteFor = null;
      onListChanged();
      void reload();
    }}
  />
{/if}
