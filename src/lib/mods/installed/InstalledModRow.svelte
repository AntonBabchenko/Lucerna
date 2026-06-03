<script lang="ts">
  import type {
    DepRoot,
    DepTreeNode,
    InstalledMod,
    ModSource,
    ModSummary,
    ModUpdateState,
  } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import ModCard from '../ModCard.svelte';
  import DepSection from './DepSection.svelte';
  import type { RequiredByEntry } from './dep-graph.svelte';

  let {
    summary,
    installed,
    rowKey,
    root,
    requiredBy,
    depTotal,
    depMissing,
    expanded,
    graphLoading,
    hoveredKey,
    updateState,
    checking,
    packChip,
    selected,
    onToggleExpand,
    onHover,
    onOpenDetail,
    onOpenDetailMod,
    onToggle,
    onUninstall,
    onUpdate,
    onSelectChange,
    onInstallDep,
    onJump,
  }: {
    summary: ModSummary | null;
    installed: InstalledMod;
    rowKey: string;
    root: DepRoot | undefined;
    requiredBy: RequiredByEntry[];
    depTotal: number;
    depMissing: number;
    expanded: boolean;
    graphLoading: boolean;
    hoveredKey: string | null;
    updateState: ModUpdateState | null;
    checking: boolean;
    packChip: string | null;
    selected: boolean;
    onToggleExpand: () => void;
    onHover: (k: string | null) => void;
    // The MAIN row's own ModCard detail opener.
    onOpenDetail: () => void;
    // Opens the info modal for any dependency mod by (source, project_id).
    onOpenDetailMod: (source: ModSource, projectId: string) => void;
    onToggle: () => void;
    onUninstall: () => void;
    onUpdate: () => void;
    onSelectChange: (checked: boolean) => void;
    onInstallDep: (node: DepTreeNode) => void;
    onJump: (node: DepTreeNode) => void;
  } = $props();

  // Single status badge per row, highest priority first (spec §4.4):
  // missing deps → update available → disabled → none.
  const badge = $derived.by(() => {
    if (depMissing > 0)
      return {
        kind: 'missing' as const,
        text: $t('mods.installed.badgeMissing', { count: depMissing }),
      };
    if (updateState?.kind === 'update_available')
      return {
        kind: 'update' as const,
        text: $t('mods.installed.badgeUpdate', { version: updateState.target.version_number }),
      };
    if (!installed.enabled) return { kind: 'off' as const, text: $t('mods.installed.badgeOff') };
    return null;
  });
</script>

<div role="group" aria-label={installed.name}>
  <!-- Hover region = the mod row + its chip line ONLY. The expanded DepSection
       is a sibling below, so its per-node hover doesn't fight the row's hover
       over the shared hoveredKey. -->
  <div
    data-mod-key={rowKey}
    data-mod-row={rowKey}
    class:bg-highlight={hoveredKey === rowKey}
    onmouseenter={() => onHover(rowKey)}
    onmouseleave={() => onHover(null)}
    role="presentation"
  >
    <ModCard
      layout="list"
      dense={true}
      highlighted={hoveredKey === rowKey}
      {summary}
      {installed}
      onInstall={() => {}}
      {onOpenDetail}
      {onToggle}
      {onUninstall}
      {updateState}
      {onUpdate}
      {checking}
      {packChip}
      selectable={true}
      {selected}
      {onSelectChange}
    />
    {#if summary}
      <div class="flex items-center gap-2 px-3 pb-0.5 text-xs">
        {#if badge}
          <span
            data-testid="status-badge"
            class="px-2 py-0.5 rounded {badge.kind === 'missing'
              ? 'bg-danger-bg text-danger'
              : badge.kind === 'update'
                ? 'bg-warning-bg text-warning-text'
                : 'bg-subtle text-muted'}">{badge.text}</span
          >
        {/if}
        {#if graphLoading && !root}
          <span class="text-placeholder">{$t('mods.installed.resolvingShort')}</span>
        {:else}
          {#if depTotal > 0}
            <button
              type="button"
              class="px-2 py-0.5 rounded bg-accent-soft text-accent"
              onclick={onToggleExpand}
            >
              {expanded ? '▾' : '▸'}
              {$t('mods.installed.depCount', { count: depTotal })}{depMissing > 0
                ? ` · ${$t('mods.installed.depMissing', { count: depMissing })}`
                : ''}
            </button>
          {/if}
          {#if requiredBy.length > 0}
            <button
              type="button"
              class="px-2 py-0.5 rounded bg-subtle text-secondary"
              onclick={onToggleExpand}
              >{$t('mods.installed.requiredByCount', { count: requiredBy.length })}</button
            >
          {/if}
        {/if}
      </div>
    {/if}
  </div>
  {#if summary && expanded && root}
    <DepSection
      {root}
      {requiredBy}
      {hoveredKey}
      {onHover}
      onInstall={onInstallDep}
      {onJump}
      onOpenDetail={onOpenDetailMod}
    />
  {/if}
</div>
