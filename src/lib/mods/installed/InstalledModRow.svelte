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
  import { Icon } from '$lib/ui/icons';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import { browserPrefs } from '../browser-prefs.svelte';
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
    incompatibleTitle,
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
    incompatibleTitle: string | null;
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
    onJump: (target: { source: ModSource; project_id: string }) => void;
  } = $props();

  // One expand control summarises both directions of the dependency relation:
  // what this mod requires AND what requires it. Both share a single panel
  // (DepSection), so a single chip / single toggle is the honest control. The
  // pieces are joined with " · " (e.g. "1 dep · required by 2").
  const expandLabel = $derived.by(() => {
    const parts: string[] = [];
    if (depTotal > 0) {
      let s = $t('mods.installed.depCount', { count: depTotal });
      if (depMissing > 0) s += ` · ${$t('mods.installed.depMissing', { count: depMissing })}`;
      parts.push(s);
    }
    if (requiredBy.length > 0)
      parts.push($t('mods.installed.requiredByCount', { count: requiredBy.length }));
    return parts.join(' · ');
  });

  // The only left-side row badge is the missing-dependency warning — it is
  // row-specific and shown nowhere else. The "update available" and "disabled"
  // states are intentionally NOT badged here: the ModCard on the right already
  // shows the version transition (vOld → vNew) + Update button and the
  // enable/disable control + "Installed" chip, so a left badge would duplicate
  // them.
  const badge = $derived.by(() => {
    if (depMissing > 0) return { text: $t('mods.installed.badgeMissing', { count: depMissing }) };
    return null;
  });
</script>

<div role="group" aria-label={installed.name}>
  <!-- Hover region = the mod row + its chip line ONLY. The expanded DepSection
       is a sibling below, so its per-node hover doesn't fight the row's hover
       over the shared hoveredKey. -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    data-mod-key={rowKey}
    data-mod-row={rowKey}
    class:bg-highlight={hoveredKey === rowKey}
    onmouseenter={() => onHover(rowKey)}
    onmouseleave={() => onHover(null)}
  >
    <ModCard
      layout="list"
      density={browserPrefs.density}
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
    {#if summary || incompatibleTitle}
      <div class="flex items-center gap-2 px-3 pb-0.5 text-xs">
        {#if incompatibleTitle}
          <span data-testid="incompat-badge" use:tooltip={incompatibleTitle}>
            <StatusBadge variant="warning" icon="warning">
              {$t('mods.installed.badgeIncompatible')}
            </StatusBadge>
          </span>
        {/if}
        {#if badge}
          <span data-testid="status-badge" use:tooltip={$t('mods.installed.missingDepsTooltip')}>
            <StatusBadge variant="danger" icon="warning">{badge.text}</StatusBadge>
          </span>
        {/if}
        {#if graphLoading && !root}
          <span class="text-placeholder">{$t('mods.installed.resolvingShort')}</span>
        {:else if depTotal > 0 || requiredBy.length > 0}
          <!-- Single toggle for the whole relation. Accent (actionable) when the
               mod has its own deps; muted when it is only required-by. -->
          <button
            type="button"
            class="px-2 py-0.5 rounded inline-flex items-center gap-1.5 {depTotal > 0
              ? 'bg-accent-soft text-accent'
              : 'bg-subtle text-secondary'}"
            onclick={onToggleExpand}
          >
            <Icon name={expanded ? 'chevronDown' : 'caret'} />
            {expandLabel}
          </button>
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
