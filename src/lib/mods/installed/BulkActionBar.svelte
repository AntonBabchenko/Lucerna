<script lang="ts">
  import { t } from '$lib/i18n';

  let {
    allSelected,
    selectedCount,
    indeterminate,
    busy,
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
    busy: boolean;
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
  <input
    type="checkbox"
    class="flex-shrink-0"
    aria-label={$t('mods.installed.selectAll')}
    checked={allSelected}
    {indeterminate}
    onchange={(e) => onToggleAll((e.currentTarget as HTMLInputElement).checked)}
  />
  {#if selectedCount > 0}
    <span class="font-medium text-accent"
      >{$t('mods.installed.selectedCount', { count: selectedCount })}</span
    >
    <div data-testid="bulk-bar" class="ml-auto flex items-center gap-1">
      <button type="button" class="btn-secondary btn-xs" disabled={busy} onclick={onEnable}
        >{$t('mods.card.enable')}</button
      >
      <button type="button" class="btn-secondary btn-xs" disabled={busy} onclick={onDisable}
        >{$t('mods.card.disable')}</button
      >
      <button
        type="button"
        class="btn-secondary btn-xs"
        disabled={busy || !canUpdate}
        title={!canUpdate ? $t('mods.installed.bulkUpdateTitle') : ''}
        onclick={onUpdate}>{$t('mods.card.update')}</button
      >
      <button type="button" class="btn-ghost-danger btn-xs" disabled={busy} onclick={onUninstall}
        >{$t('mods.card.uninstall')}</button
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
