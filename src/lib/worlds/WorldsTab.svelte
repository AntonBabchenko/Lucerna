<script lang="ts">
  import { commands, events, type World } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { relativeTime } from '$lib/format/relative-time';
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import { WORLDS_STEPS } from '$lib/onboarding/contextual-tours';
  import BackupsDialog from '$lib/worlds/BackupsDialog.svelte';
  import DeleteWorldDialog from '$lib/worlds/DeleteWorldDialog.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { Icon } from '$lib/ui/icons';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { t } from '$lib/i18n';

  let {
    instanceId,
    onListChanged,
  }: {
    instanceId: string | null;
    onListChanged: () => void;
  } = $props();

  let worlds = $state<World[]>([]);
  let listError = $state<string | null>(null);
  let loading = $state(false);
  let openMenuFor = $state<string | null>(null);
  let menuTop = $state(0);
  let menuLeft = $state(0);
  let backupsFor = $state<World | null>(null);
  let deleteFor = $state<World | null>(null);

  // The MainTabs content area is overflow-y:auto which (per the CSS
  // overflow spec) forces overflow-x:auto too, clipping any
  // absolute-positioned popover that extends past the area's bottom or
  // right. Same fix as the sidebar (?)-tooltip in
  // InstanceConceptTooltip.svelte: render the kebab menu position:fixed
  // with coords measured from the trigger on open, then close on
  // scroll/resize so a fixed popover never drifts from a moved trigger.
  const MENU_WIDTH = 192; // = Tailwind w-48
  const GAP = 4;
  const MARGIN = 8;

  function toggleMenu(folderName: string, e: MouseEvent) {
    if (openMenuFor === folderName) {
      openMenuFor = null;
      return;
    }
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menuTop = r.bottom + GAP;
    const wantLeft = r.right - MENU_WIDTH; // right-align with trigger
    const maxLeft = window.innerWidth - MENU_WIDTH - MARGIN;
    menuLeft = Math.min(Math.max(wantLeft, MARGIN), Math.max(MARGIN, maxLeft));
    openMenuFor = folderName;
  }

  $effect(() => {
    if (openMenuFor == null) return;
    const close = () => (openMenuFor = null);
    // capture-phase so the MainTabs scroll container also fires (scroll
    // events do not bubble).
    window.addEventListener('scroll', close, true);
    window.addEventListener('resize', close);
    return () => {
      window.removeEventListener('scroll', close, true);
      window.removeEventListener('resize', close);
    };
  });

  async function reload() {
    if (!instanceId) {
      worlds = [];
      return;
    }
    loading = true;
    listError = null;
    const r = await commands.listWorlds(instanceId);
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
  // Subscribe-and-cleanup so multiple WorldsTab mounts don't pile up
  // listeners.
  $effect(() => {
    let unlisten: (() => void) | null = null;
    void events.processExited
      .listen(() => void reload())
      .then((u) => {
        unlisten = u;
      });
    return () => {
      if (unlisten) unlisten();
    };
  });

  async function onBackupNow(w: World) {
    if (!instanceId) return;
    openMenuFor = null;
    const r = await commands.backupWorld(instanceId, w.folder_name);
    if (r.status === 'ok') {
      pushSuccess(
        $t('worlds.tab.toastBackedUp', {
          name: w.folder_name,
          size: formatBytes(r.data.size_bytes),
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

  function formatBytes(n: number | null | undefined): string {
    if (n == null) return '';
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
    return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
  }
</script>

<div class="p-3 flex flex-col gap-2" data-testid="worlds-tab">
  {#if !instanceId}
    <p class="text-sm text-muted">{$t('worlds.tab.noInstance')}</p>
  {:else if loading}
    <div class="flex justify-center py-8 text-secondary">
      <Spinner delayMs={150} label={$t('worlds.tab.loading')} />
    </div>
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
        <li>
          <button
            type="button"
            class="w-full flex items-center justify-between gap-2 px-3 py-2 text-left hover:bg-subtle"
            aria-label={$t('worlds.tab.worldActionsAriaLabel', { name: w.folder_name })}
            aria-expanded={openMenuFor === w.folder_name}
            onclick={(e) => toggleMenu(w.folder_name, e)}
          >
            <div class="min-w-0">
              <div class="flex items-center gap-2">
                <span class="font-medium truncate">{w.folder_name}</span>
                {#if w.backup_count > 0}
                  <span
                    class="inline-flex items-center gap-1 text-xs text-warning-text bg-warning-bg rounded px-1.5 py-0.5"
                    aria-label={$t('worlds.tab.backupCountAriaLabel', { count: w.backup_count })}
                  >
                    <Icon name="package" size={12} />
                    {w.backup_count}
                  </span>
                {/if}
              </div>
              <div class="text-xs text-muted">
                {formatBytes(w.size_bytes)} · {relativeTime(w.modified_unix_ms)}
              </div>
            </div>
            <span class="text-placeholder flex-shrink-0" aria-hidden="true"
              ><Icon name="moreVertical" size={16} /></span
            >
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  <button
    type="button"
    class="btn-tertiary self-start inline-flex items-center gap-1"
    data-tour-ctx="worlds-open-folder"
    onclick={() => void onOpenSavesFolder()}
  >
    {$t('worlds.tab.openSavesFolder')}
    <Icon name="externalLink" size={14} />
  </button>

  <!-- Tour fires only once worlds exist — most steps point at the
       list which is absent on a fresh instance. -->
  {#if worlds.length > 0}
    <ContextualTour id="worlds" steps={WORLDS_STEPS} />
  {/if}
</div>

{#if openMenuFor}
  {@const activeWorld = worlds.find((x) => x.folder_name === openMenuFor)}
  {#if activeWorld}
    <!-- Click-outside backdrop -->
    <button
      type="button"
      class="fixed inset-0 z-40"
      aria-label={$t('worlds.tab.closeMenuAriaLabel')}
      onclick={() => (openMenuFor = null)}
    ></button>
    <div
      class="fixed z-50 w-48 bg-surface border border-border-subtle rounded shadow"
      style="top: {menuTop}px; left: {menuLeft}px;"
      role="menu"
    >
      <button
        type="button"
        role="menuitem"
        class="block w-full text-left px-3 py-2 text-sm hover:bg-subtle"
        onclick={() => void onBackupNow(activeWorld)}
      >
        {$t('worlds.tab.backupNow')}
      </button>
      <button
        type="button"
        role="menuitem"
        class="block w-full text-left px-3 py-2 text-sm hover:bg-subtle"
        onclick={() => {
          backupsFor = activeWorld;
          openMenuFor = null;
        }}
      >
        {$t('worlds.tab.viewBackups')}
      </button>
      <button
        type="button"
        role="menuitem"
        class="block w-full text-left px-3 py-2 text-sm hover:bg-subtle text-danger"
        onclick={() => {
          deleteFor = activeWorld;
          openMenuFor = null;
        }}
      >
        {$t('worlds.tab.deleteWorld')}
      </button>
    </div>
  {/if}
{/if}

{#if backupsFor && instanceId}
  <BackupsDialog
    {instanceId}
    world={backupsFor}
    onClose={() => {
      backupsFor = null;
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
