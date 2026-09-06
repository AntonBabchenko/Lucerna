<script lang="ts">
  import type { World } from '$lib/ipc/bindings';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import TabBar from '$lib/ui/TabBar.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import BackupsPanel from '$lib/worlds/BackupsPanel.svelte';
  import WorldDatapacks from '$lib/worlds/WorldDatapacks.svelte';
  import { t } from '$lib/i18n';

  // Per-world detail dialog — the world row's single entry point, covering
  // both per-world concerns (Backups, Datapacks) behind one TabBar instead of
  // bolting a fourth inline row icon onto an already-crowded row. Backups used
  // to be its own dialog (BackupsDialog, now retired): that content lives in
  // BackupsPanel so it can sit inside this shared chrome instead of nesting a
  // second Modal (Modal's shared openStack only gives Escape to the topmost
  // layer, so two stacked Modals for one logical surface would be wrong).
  //
  // Migration to another instance (world-migration spec §7, A12) lives here
  // for the same reason: a footer action beside the tabs, not a fourth row
  // icon. The dialog only reports the click — WorldsTab owns the
  // MigrateWorldDialog because it owns the world list the outcome changes.

  let {
    instanceId,
    world,
    running = false,
    migrateDisabledReason = null,
    onClose,
    onChanged,
    onMigrate = () => {},
  }: {
    instanceId: string;
    world: World;
    running?: boolean;
    /** Why the migrate action is unavailable right now (data root fallen
     *  back, source running) — rendered as its tooltip; `null` = enabled. */
    migrateDisabledReason?: string | null;
    onClose: () => void;
    onChanged: () => void;
    onMigrate?: () => void;
  } = $props();

  type TabId = 'backups' | 'datapacks';
  let tab = $state<TabId>('backups');
</script>

<Modal
  ariaLabelledby="world-detail-title"
  {onClose}
  dataTestid="world-detail-dialog"
  panelClass="w-full max-w-2xl max-h-[85vh] flex flex-col"
>
  <!-- Fixed header: title, tabs stay put while the active tab's body scrolls. -->
  <div class="p-4 pb-0 shrink-0">
    <div class="flex items-start justify-between gap-2">
      <h2
        id="world-detail-title"
        class="text-base font-semibold text-primary flex-1 min-w-0 truncate"
      >
        {world.folder_name}
      </h2>
      <CloseButton onClick={onClose} />
    </div>
    <div class="mt-3">
      <TabBar
        tabs={[
          { id: 'backups', label: $t('worlds.backups.tab') },
          { id: 'datapacks', label: $t('worlds.datapacks.tab') },
        ]}
        active={tab}
        ariaLabel={$t('worlds.detail.tabsLabel')}
        panelId="world-detail-panel"
        onChange={(id) => (tab = id as TabId)}
      />
    </div>
  </div>

  <!-- Scrollable body. role=tabpanel + id + aria-labelledby completes the
       WAI-ARIA tabs wiring TabBar's panelId prop sets up on the tab side
       (aria-controls + each tab's own id, formula `${panelId}-tab-${tab.id}`). -->
  <div
    class="flex-1 overflow-y-auto min-h-0 p-4 pt-3"
    role="tabpanel"
    id="world-detail-panel"
    aria-labelledby="world-detail-panel-tab-{tab}"
  >
    {#if tab === 'backups'}
      <BackupsPanel {instanceId} worldFolder={world.folder_name} {onClose} {onChanged} />
    {:else}
      <WorldDatapacks {instanceId} world={world.folder_name} {running} />
    {/if}
  </div>

  <!-- Fixed footer: the migrate entry point. Text-labelled `.btn-secondary`
       (DESIGN.md §5 — a committing, standalone dialog action carries no icon).
       A disabled button fires no pointer events and `title=` is banned, so the
       reason rides the wrapping span's tooltip, and the span is focusable only
       while the action is blocked so the explanation is reachable by keyboard
       — the OverviewTab / WorldDatapacks shape. -->
  <div class="p-4 pt-2 shrink-0 border-t border-border-subtle flex justify-end">
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <span
      class="inline-flex"
      tabindex={migrateDisabledReason !== null ? 0 : undefined}
      use:tooltip={{ text: migrateDisabledReason ?? '', describe: false }}
    >
      <button
        type="button"
        class="btn-secondary btn-sm"
        data-testid="world-migrate-btn"
        disabled={migrateDisabledReason !== null}
        onclick={onMigrate}
      >
        {$t('worlds.migrate.entry.action')}
      </button>
    </span>
  </div>
</Modal>
