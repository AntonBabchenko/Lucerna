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
  import Spinner from '$lib/ui/Spinner.svelte';
  import ModCard from '../ModCard.svelte';
  import DepSection from './DepSection.svelte';
  import type { RequiredByEntry } from './dep-graph.svelte';
  import { isClaimDismissed } from '$lib/mods/dep-claim-dismiss';
  import { changelogSupported } from '$lib/mods/changelog-supported';

  let {
    summary,
    installed,
    rowKey,
    root,
    requiredBy,
    depTotal,
    hasPreflightIssue,
    expanded,
    graphLoading,
    hoveredKey,
    updateState,
    checking,
    packChip,
    incompatibleTitle,
    selected,
    outOfRangeKeys = new Set(),
    onToggleExpand,
    onHover,
    onOpenDetail,
    onOpenDetailMod,
    onToggle,
    onUninstall,
    onUpdate,
    onShowChangelog,
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
    // This mod is the dependent in a pre-flight violation — i.e. the LOADER
    // will not get what it needs. The dependency graph cannot answer this; it
    // only knows what the platform was told.
    hasPreflightIssue: boolean;
    expanded: boolean;
    graphLoading: boolean;
    hoveredKey: string | null;
    updateState: ModUpdateState | null;
    checking: boolean;
    packChip: string | null;
    incompatibleTitle: string | null;
    selected: boolean;
    outOfRangeKeys?: Set<string>;
    onToggleExpand: () => void;
    onHover: (k: string | null) => void;
    // The MAIN row's own ModCard detail opener.
    onOpenDetail: () => void;
    // Opens the info modal for any dependency mod by (source, project_id).
    onOpenDetailMod: (source: ModSource, projectId: string) => void;
    onToggle: () => void;
    onUninstall: () => void;
    onUpdate: () => void;
    // Opens the cumulative changelog for the pending update. Only invoked when
    // `showChangelog` below is true (update available + supported source).
    onShowChangelog: () => void;
    onSelectChange: (checked: boolean) => void;
    onInstallDep: (node: DepTreeNode) => void;
    onJump: (target: { source: ModSource; project_id: string }) => void;
  } = $props();

  // One expand control summarises both directions of the dependency relation:
  // what this mod requires AND what requires it. Both share a single panel
  // (DepSection), so a single chip / single toggle is the honest control. The
  // pieces are joined with " · " (e.g. "1 dep · required by 2").
  const optionalTotal = $derived(root?.optional.length ?? 0);

  const expandLabel = $derived.by(() => {
    const parts: string[] = [];
    // Relationship count only. The "· N missing" suffix is gone with the graph's
    // verdict: it counted absences the loader may never have asked for.
    if (depTotal > 0) parts.push($t('mods.installed.depCount', { count: depTotal }));
    if (requiredBy.length > 0)
      parts.push($t('mods.installed.requiredByCount', { count: requiredBy.length }));
    // Last resort only, so every row that already renders a label keeps it
    // byte-identical. A mod whose required deps are all loader-scoped away (a
    // merged multi-loader jar on one of its loaders) would otherwise have no
    // label and no chip at all, taking its still-correct optional section with
    // it — the widened gate below needs something to render.
    if (parts.length === 0 && optionalTotal > 0)
      parts.push($t('mods.installed.depOptionalCount', { count: optionalTotal }));
    return parts.join(' · ');
  });

  // There is deliberately no left-side danger badge any more. The one that used
  // to live here counted the graph's absent required children — i.e. the
  // platform's claim — and a measured mod's claim was contradicted by its own
  // jar. A real problem is a pre-flight violation: the ModCard's danger accent
  // marks the row and PreflightPanel above the list carries the detail and the
  // fix. "Update available" and "disabled" were already unbadged here because
  // the ModCard on the right shows both.

  // Dependencies the AUTHOR marked required on the platform that are not
  // installed and the user has not settled. Reported, not asserted: the loader
  // enforces only what the jar descriptor declares, and a measured mod's
  // platform entry is contradicted by its own `neoforge.mods.toml`. Counting
  // only top-level `required` children keeps the badge about this mod's own
  // claims rather than its dependencies' claims.
  const claimRefs = $derived(
    installed.source && installed.project_id
      ? { source: installed.source, project_id: installed.project_id }
      : null,
  );
  const authorClaims = $derived(
    claimRefs === null
      ? []
      : (root?.required ?? []).filter(
          (n) =>
            !n.installed &&
            n.declared === 'required' &&
            !isClaimDismissed(claimRefs, { source: n.source, project_id: n.project_id }),
        ),
  );

  // "View changelog" is offered only when an update is actually pending and the
  // source implements a changelog API (Modrinth/CurseForge) — mirrors the Rust
  // `changelog_supported` gate. Guards on identity so the modal always has a
  // (source, project_id, base version) to query.
  const showChangelog = $derived(
    updateState?.kind === 'update_available' &&
      !!installed.source &&
      changelogSupported(installed.source) &&
      !!installed.project_id &&
      !!installed.version_id,
  );
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
      attention={hasPreflightIssue ? 'missing-deps' : incompatibleTitle ? 'incompatible' : null}
      selectable={true}
      {selected}
      {onSelectChange}
    />
    {#if summary || incompatibleTitle || showChangelog || authorClaims.length > 0}
      <div class="flex items-center gap-2 px-3 pb-0.5 text-xs">
        {#if incompatibleTitle}
          <!-- Badge only: the remediation entry is the single instance-wide
               "Fix incompatible mods" button in the compat panel header. A
               per-row button here opened that same unscoped dialog, so N of
               them were N identical controls posing as a per-mod action. -->
          <span data-testid="incompat-badge" use:tooltip={incompatibleTitle}>
            <StatusBadge variant="warning" icon="warning">
              {$t('mods.installed.badgeIncompatible')}
            </StatusBadge>
          </span>
        {/if}
        {#if authorClaims.length > 0}
          <!-- Neutral register on purpose: not `danger`, not `warning`. Nothing
               is being demanded of the user — the launcher is reporting what the
               author typed, and saying whose word it is. -->
          <span
            class="px-2 py-0.5 rounded inline-flex items-center gap-1 bg-subtle text-secondary"
            use:tooltip={$t('mods.deps.authorClaimTooltip')}
            data-testid="author-claim-badge"
          >
            <Icon name="info" size={12} />
            {$t('mods.deps.authorClaimCount', { count: authorClaims.length })}
          </span>
        {/if}
        {#if showChangelog}
          <button
            type="button"
            class="px-2 py-0.5 rounded inline-flex items-center gap-1 bg-subtle text-secondary"
            onclick={onShowChangelog}
            data-testid="mod-changelog-btn"
          >
            <Icon name="scrollText" />
            {$t('mods.changelog.view')}
          </button>
        {/if}
        {#if graphLoading && !root}
          <span class="text-placeholder">
            <Spinner
              size="sm"
              labelPlacement="right"
              label={$t('mods.installed.resolvingShort')}
              delayMs={150}
            />
          </span>
        {:else if depTotal > 0 || optionalTotal > 0 || requiredBy.length > 0}
          <!-- Single toggle for the whole relation. Accent (actionable) when the
               mod has its own deps; muted when it is only required-by.
               `optionalTotal` is in the condition because it is the sole reason
               the panel may still be worth opening once every required dep has
               been loader-scoped away. -->
          <button
            type="button"
            data-testid="dep-expand-chip"
            aria-expanded={expanded}
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
      {outOfRangeKeys}
    />
  {/if}
</div>
