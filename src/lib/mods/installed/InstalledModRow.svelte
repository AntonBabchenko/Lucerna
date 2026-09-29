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

  // One pill summarises both directions of the dependency relation (spec D12): ⛓ what this mod
  // requires (distinct projects) and ↑ what requires it. Both share a single panel (DepSection),
  // so a single toggle is the honest control. Its figures are in its name and tooltip, joined
  // with " · " (e.g. "1 dep · required by 2").
  const optionalTotal = $derived(root?.optional.length ?? 0);
  // The platform could not describe this mod's installed version: no count is not zero.
  const depsUnknown = $derived(root?.deps_unknown ?? null);

  // `unknown` words the unknown requirements: the short status in the name, the reason in the
  // tooltip — the only place the pill can say why.
  function relationParts(unknown: string): string[] {
    const parts: string[] = [];
    if (depsUnknown) parts.push(unknown);
    // Relationship count only. The "· N missing" suffix is gone with the graph's
    // verdict: it counted absences the loader may never have asked for.
    if (depTotal > 0) parts.push($t('mods.installed.depCount', { count: depTotal }));
    if (requiredBy.length > 0)
      parts.push($t('mods.installed.requiredByCount', { count: requiredBy.length }));
    // Last resort only. A mod whose required deps are all loader-scoped away (a
    // merged multi-loader jar on one of its loaders) would otherwise have no
    // label and no pill at all, taking its still-correct optional section with
    // it — the widened gate below needs something to render.
    if (parts.length === 0 && optionalTotal > 0)
      parts.push($t('mods.installed.depOptionalCount', { count: optionalTotal }));
    return parts;
  }
  const expandLabel = $derived(relationParts($t('mods.deps.depsUnknownStatus')).join(' · '));
  const relationTooltip = $derived(
    depsUnknown ? relationParts($t(DEPS_UNKNOWN_KEY[depsUnknown])).join(' · ') : expandLabel,
  );
  // Only an enabled platform mod becomes a root of the graph: no other row waits for it.
  const mayHaveRoot = $derived(installed.enabled && !!installed.source && !!installed.project_id);
  const relationLoading = $derived(graphLoading && !root && mayHaveRoot);
  const hasRelation = $derived(
    !!depsUnknown || depTotal > 0 || optionalTotal > 0 || requiredBy.length > 0,
  );

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

  // The update badge opens the changelog only when an update is actually pending
  // and the source implements a changelog API (Modrinth/CurseForge) — mirrors the
  // Rust `changelog_supported` gate; otherwise it stays a static badge. Guards on
  // identity so the modal always has a (source, project_id, base version) to query.
  const showChangelog = $derived(
    updateState?.kind === 'update_available' &&
      !!installed.source &&
      changelogSupported(installed.source) &&
      !!installed.project_id &&
      !!installed.version_id,
  );
</script>

{#snippet relationPill()}
  {#if relationLoading}
    <Spinner
      size="sm"
      class="text-placeholder"
      label={$t('mods.installed.resolvingShort')}
      delayMs={150}
    />
  {:else}
    <!-- One neutral pill for both directions (spec D12): ⛓ own dependencies (by project; «?»
         while the platform could not describe them — unknown is not zero) and ↑ dependents.
         The words live in its name and tooltip; accent only while its panel is open. The ⛓
         figure falls back to the optional count when that is all there is to open. -->
    <button
      type="button"
      data-testid="relation-pill"
      aria-expanded={expanded}
      aria-label={expandLabel}
      use:tooltip={{ text: relationTooltip, describe: !!depsUnknown }}
      class="px-1.5 py-0.5 rounded inline-flex items-center gap-2 text-xs tabular-nums {expanded
        ? 'bg-accent-soft text-accent'
        : 'bg-subtle text-secondary'}"
      onclick={onToggleExpand}
    >
      {#if depsUnknown || depTotal > 0 || requiredBy.length === 0}
        <span class="inline-flex items-center gap-0.5"
          ><Icon name="link" size={12} />{depsUnknown
            ? '?'
            : depTotal > 0
              ? depTotal
              : optionalTotal}</span
        >
      {/if}
      {#if requiredBy.length > 0}
        <span class="inline-flex items-center gap-0.5"
          ><Icon name="arrowUp" size={12} />{requiredBy.length}</span
        >
      {/if}
    </button>
  {/if}
{/snippet}

<div role="group" aria-label={installed.name}>
  <!-- Hover region = the mod row + its problem line ONLY. The expanded
       DepSection is a sibling below, so its per-node hover doesn't fight the
       row's hover over the shared hoveredKey. It draws the cross-highlight
       once, as a ring above the card and its problem line (`relative`: the
       ring's containing block). -->
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
      relation={relationLoading || hasRelation ? relationPill : undefined}
    />
    <!-- The second line is there only for a problem (spec D12): what used to sit here — the
         changelog chip and the dependency chip — is the update badge and the relation pill in
         the row now. -->
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
  </div>
  <!-- A row whose platform details did not load still has its dependencies: the panel needs the
       graph's root, not the summary. -->
  {#if expanded && root}
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
