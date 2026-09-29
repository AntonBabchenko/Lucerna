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
  import { tooltip } from '$lib/ui/tooltip';
  import Spinner from '$lib/ui/Spinner.svelte';
  import ModCard from '../ModCard.svelte';
  import DepSection from './DepSection.svelte';
  import type { RequiredByEntry } from './dep-graph.svelte';
  import { DEPS_UNKNOWN_KEY, type DepTreeCtx, EMPTY_TREE_CTX } from '../dep-node-state';
  import { changelogSupported } from '$lib/mods/changelog-supported';
  import type { RowFix, RowProblem } from './row-problem';

  let {
    summary,
    installed,
    rowKey,
    root,
    requiredBy,
    depTotal,
    problem = null,
    expanded,
    graphLoading,
    hoveredKey,
    updateState,
    held = false,
    checking,
    packChip,
    selected,
    outOfRangeKeys = new Set(),
    treeCtx = EMPTY_TREE_CTX,
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
    onProblemFix = () => {},
    onRevealProblems = () => {},
  }: {
    summary: ModSummary | null;
    installed: InstalledMod;
    rowKey: string;
    root: DepRoot | undefined;
    requiredBy: RequiredByEntry[];
    depTotal: number;
    // The second line (spec §6.2): the first reason this mod blocks the game
    // (a pre-flight violation where it is the dependent — the LOADER will not
    // get what it needs; the dependency graph cannot tell) or may not work (its
    // compat warning), with at most one fix. Null = nothing wrong with it.
    problem?: RowProblem | null;
    expanded: boolean;
    graphLoading: boolean;
    hoveredKey: string | null;
    updateState: ModUpdateState | null;
    // Updates for this project are held («Не обновлять»): the card pins its version.
    held?: boolean;
    checking: boolean;
    packChip: string | null;
    selected: boolean;
    outOfRangeKeys?: Set<string>;
    // What the expanded tree needs to say what the loader does about each dependency.
    treeCtx?: DepTreeCtx;
    onToggleExpand: () => void;
    onHover: (k: string | null) => void;
    // The MAIN row's own ModCard detail opener.
    onOpenDetail: () => void;
    // Opens the info modal for any dependency mod by (source, project_id).
    onOpenDetailMod: (source: ModSource, projectId: string) => void;
    onToggle: () => void;
    onUninstall: () => void;
    onUpdate: () => void;
    // Opens the cumulative changelog for the pending update — from the card's
    // update badge. Only wired when `showChangelog` below is true (update
    // available + supported source).
    onShowChangelog: () => void;
    onSelectChange: (checked: boolean) => void;
    // A dependency's Install / Add in the tree, with the mod that declared it (null under an
    // absent parent).
    onInstallDep: (node: DepTreeNode, dependentSha1: string | null) => void;
    onJump: (target: { source: ModSource; project_id: string }) => void;
    onProblemFix?: (fix: RowFix) => void;
    // «and N more»: reveal this mod's rows in the «What stops the game» panel.
    onRevealProblems?: () => void;
  } = $props();

  // One expand control summarises both directions of the dependency relation:
  // what this mod requires AND what requires it. Both share a single panel
  // (DepSection), so a single chip / single toggle is the honest control. The
  // pieces are joined with " · " (e.g. "1 dep · required by 2").
  const optionalTotal = $derived(root?.optional.length ?? 0);
  // The platform could not describe this mod's installed version: no count is not zero.
  const depsUnknown = $derived(root?.deps_unknown ?? null);

  const expandLabel = $derived.by(() => {
    const parts: string[] = [];
    // Said where the count would be — the panel it opens says why.
    if (depsUnknown) parts.push($t('mods.deps.depsUnknownStatus'));
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
  // jar. A real problem is a pre-flight violation (or, amber, a compat
  // warning): the ModCard's accent strip marks the row, the problem line under
  // it names the first reason with its one fix, and PreflightPanel above the
  // list carries every blocking reason. "Update available" and "disabled" were
  // already unbadged here because the ModCard on the right shows both.

  // The accent strip follows the problem's level: red stops the game, amber
  // may not work (spec D6).
  const attention = $derived(
    problem?.level === 'blocking'
      ? ('missing-deps' as const)
      : problem?.level === 'warning'
        ? ('incompatible' as const)
        : null,
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
  <!-- Hover region = the mod row + its problem and chip lines ONLY. The
       expanded DepSection is a sibling below, so its per-node hover doesn't
       fight the row's hover over the shared hoveredKey. It draws the
       cross-highlight once, as a ring above the card and both lines
       (`relative`: the ring's containing block). -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    data-mod-key={rowKey}
    data-mod-row={rowKey}
    class="relative"
    class:dep-highlight={hoveredKey === rowKey}
    onmouseenter={() => onHover(rowKey)}
    onmouseleave={() => onHover(null)}
  >
    <ModCard
      layout="list"
      {summary}
      {installed}
      onInstall={() => {}}
      {onOpenDetail}
      {onToggle}
      {onUninstall}
      {updateState}
      {onUpdate}
      onShowChangelog={showChangelog ? onShowChangelog : null}
      {held}
      {checking}
      {packChip}
      {attention}
      selectable={true}
      {selected}
      {onSelectChange}
    />
    {#if problem}
      {@const tone = problem.level === 'blocking' ? 'text-danger' : 'text-warning-text'}
      <!-- One reason, in full (it wraps rather than truncates: it is the
           reason, not a label), and the one fix its status chose. A warning
           keeps the longer compat sentence as its tooltip. -->
      <div
        class="flex flex-wrap items-center gap-x-2 gap-y-0.5 px-3 pb-1 text-xs"
        data-testid="row-problem"
        data-level={problem.level}
      >
        <Icon
          name={problem.level === 'blocking' ? 'circleX' : 'warning'}
          size={14}
          class="shrink-0 {tone}"
        />
        <span class="min-w-0 {tone}" use:tooltip={problem.tooltip}>{problem.text}</span>
        {#if problem.more > 0}
          <button
            type="button"
            class="btn-link text-xs shrink-0"
            data-testid="row-problem-more"
            onclick={() => onRevealProblems()}
          >
            {$t('mods.installed.reasonMore', { count: problem.more })}
          </button>
        {/if}
        {#if problem.fix}
          {@const fix = problem.fix}
          <button
            type="button"
            class="btn-secondary btn-xs shrink-0"
            onclick={() => onProblemFix(fix)}
          >
            {fix.label}
          </button>
        {/if}
      </div>
    {/if}
    {#if summary || showChangelog}
      <div class="flex items-center gap-2 px-3 pb-0.5 text-xs">
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
        {:else if depsUnknown || depTotal > 0 || optionalTotal > 0 || requiredBy.length > 0}
          <!-- Single toggle for the whole relation. Accent (actionable) when the
               mod has its own deps; muted when it is only required-by, or when
               what it requires is unknown (the panel then says why).
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
            use:tooltip={depsUnknown ? $t(DEPS_UNKNOWN_KEY[depsUnknown]) : null}
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
      {treeCtx}
    />
  {/if}
</div>
