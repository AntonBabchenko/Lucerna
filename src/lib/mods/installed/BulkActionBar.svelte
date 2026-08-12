<script lang="ts">
  import { t } from '$lib/i18n';
  import type { BulkAction } from '$lib/mods/installed/installed-selection.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import SelectAllCheckbox from '$lib/ui/SelectAllCheckbox.svelte';
  import { tooltip } from '$lib/ui/tooltip';

  let {
    allSelected,
    selectedCount,
    indeterminate,
    busy,
    busyAction,
    canUpdate,
    onToggleAll,
    onEnable,
    onDisable,
    onUpdate,
    onUninstall,
    onClear,
  }: {
    allSelected: boolean;
    selectedCount: number;
    indeterminate: boolean;
    // Aggregate gate — disables every action while any op (bulk or sibling) runs.
    busy: boolean;
    // The specific bulk action in flight — only that button shows a spinner.
    busyAction: BulkAction | null;
    canUpdate: boolean;
    onToggleAll: (checked: boolean) => void;
    onEnable: () => void;
    onDisable: () => void;
    onUpdate: () => void;
    onUninstall: () => void;
    onClear: () => void;
  } = $props();
</script>

<div class="flex items-center gap-3 px-3 py-2 border-b border-border-subtle bg-subtle/40 text-sm">
  <SelectAllCheckbox
    {allSelected}
    {indeterminate}
    onToggle={onToggleAll}
    testid="bulk-select-all"
  />
  {#if selectedCount > 0}
    <span class="font-medium text-accent"
      >{$t('mods.installed.selectedCount', { count: selectedCount })}</span
    >
    <div data-testid="bulk-bar" class="ml-auto flex items-center gap-1">
      <BusyButton
        busy={busyAction === 'enable'}
        disabled={busy}
        class="btn-secondary btn-xs"
        onclick={onEnable}>{$t('mods.card.enable')}</BusyButton
      >
      <BusyButton
        busy={busyAction === 'disable'}
        disabled={busy}
        class="btn-secondary btn-xs"
        onclick={onDisable}>{$t('mods.card.disable')}</BusyButton
      >
      <span
        class="inline-flex"
        use:tooltip={{
          text: !canUpdate ? $t('mods.installed.bulkUpdateTitle') : '',
          describe: false,
        }}
      >
        <BusyButton
          busy={busyAction === 'update'}
          disabled={busy || !canUpdate}
          class="btn-secondary btn-xs"
          onclick={onUpdate}>{$t('mods.card.update')}</BusyButton
        >
      </span>
      <BusyButton
        busy={busyAction === 'uninstall'}
        disabled={busy}
        class="btn-ghost-danger btn-xs"
        onclick={onUninstall}>{$t('mods.card.uninstall')}</BusyButton
      >
      <!-- Clear is deliberately not gated on `busy`: deselecting is a local-only
           state reset and is safe to do while a bulk IPC op is in flight. -->
      <button type="button" class="btn-ghost btn-xs" onclick={onClear}
        >{$t('mods.installed.bulkClear')}</button
      >
    </div>
  {:else}
    <span class="text-muted text-xs">{$t('mods.installed.bulkHint')}</span>
  {/if}
</div>
