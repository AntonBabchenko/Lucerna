<script lang="ts">
  import { commands, events, type World } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { listenUntilDestroyed } from '$lib/ipc/listen';
  import { relativeTime } from '$lib/format/relative-time';
  import { formatSize } from '$lib/format/size';
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import { WORLDS_STEPS } from '$lib/onboarding/contextual-tours';
  import WorldDetailDialog from '$lib/worlds/WorldDetailDialog.svelte';
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
    onListChanged,
    onQuickPlayWorld = () => {},
    quickPlayDisabledReason = null,
    running = false,
  }: {
    instanceId: string | null;
    onListChanged: () => void;
    onQuickPlayWorld?: (folderName: string) => void;
    quickPlayDisabledReason?: string | null;
    running?: boolean;
  } = $props();

  let worlds = $state<World[]>([]);
  let listError = $state<string | null>(null);
  let loading = $state(false);
  let detailFor = $state<World | null>(null);
  let deleteFor = $state<World | null>(null);

  async function reload() {
    if (!instanceId) {
      worlds = [];
      return;
    }
    // Capture the instance this load is for; a rapid instance switch mid-fetch
    // must not commit the previous instance's worlds over the newer selection.
    const reqId = instanceId;
    loading = true;
    listError = null;
    const r = await commands.listWorlds(reqId);
    if (instanceId !== reqId) return;
    loading = false;
    if (r.status === 'ok') {
      worlds = r.data;
    } else {
      listError = formatError(r.error);
    }
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
  {:else if worlds.length === 0}
    <p class="text-sm text-muted">{$t('worlds.tab.empty')}</p>
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
