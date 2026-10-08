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
  import { fieldFlash } from '$lib/ui/field-flash';
  import { tooltip } from '$lib/ui/tooltip';
  import Spinner from '$lib/ui/Spinner.svelte';
  import ModCard from '../ModCard.svelte';
  import DepSection from './DepSection.svelte';
  import type { RequiredByEntry } from './dep-graph.svelte';
  import {
    DEPS_UNKNOWN_KEY,
    type DepTreeCtx,
    EMPTY_TREE_CTX,
    type ModTarget,
  } from '../dep-node-state';
  import { changelogSupported } from '$lib/mods/changelog-supported';
  import type { RowFix, RowProblem } from './row-problem';
  import { depSectionId, hasFigures, relationFigures, relationInput } from './relation-cell';

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
    updateState,
    held = false,
    checking,
    packChip,
    selected,
    treeCtx = EMPTY_TREE_CTX,
    onToggleExpand,
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
    onRevealFile = null,
    onOpenProjectPage = null,
    hold = null,
    flash = false,
    onFlashed = () => {},
    tourAnchor = null,
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
    updateState: ModUpdateState | null;
    // Updates for this project are held («Не обновлять»): the card pins its version.
    held?: boolean;
    checking: boolean;
    packChip: string | null;
    selected: boolean;
    // What the expanded tree needs to say what the loader does about each dependency.
    treeCtx?: DepTreeCtx;
    onToggleExpand: () => void;
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
    // «Show in the list» from the tree or «Required by» (the tree passes its node, which also says
    // whether it is installed or switched off).
    onJump: (target: ModTarget) => void;
    onProblemFix?: (fix: RowFix) => void;
    // «and N more»: reveal this mod's rows in the «What stops the game» panel.
    onRevealProblems?: () => void;
    // The row menu (spec §6.8), straight to ModCard: the jar on disk, the project page (null =
    // none known), the hold (null = no hold control here).
    onRevealFile?: (() => void) | null;
    onOpenProjectPage?: (() => void) | null;
    hold?: { held: boolean; onToggle: () => void } | null;
    // A «show in the list» just landed here: the row flashes once, as a Settings jump does, and
    // says so (the host then clears it, so the next jump is a new edge).
    flash?: boolean;
    onFlashed?: () => void;
    /** The deps tour's anchor on this row's relation cell: the tour's mod (`deps-cell`, its panel
     *  carries the block anchors too) or the library it requires (`deps-required-by`). */
    tourAnchor?: 'deps-cell' | 'deps-required-by' | null;
  } = $props();

  // One control summarises both directions of the dependency relation (spec D12): ⛓ what this mod
  // requires (distinct projects) and ↑ what requires it. Both share a single panel (DepSection),
  // so a single toggle is the honest control. It is the row's relation cell, a column of its own
  // before the icon (spec 2026-09-30); its figures are in its name and tooltip too, joined with
  // " · " (e.g. "1 dep · required by 2").
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
  // What the cell's two slots show — one rule with the width of the list's column
  // (relation-cell.ts), so no figure is ever wider than the column made room for.
  const figures = $derived(relationFigures(relationInput(root, depTotal, requiredBy.length)));
  const hasRelation = $derived(hasFigures(figures));
  // The section the cell opens, named in its `aria-controls` while rendered.
  const sectionId = $derived(depSectionId(installed.sha1));

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

{#snippet relationCell()}
  {#if relationLoading}
    <Spinner
      size="sm"
      class="text-placeholder"
      label={$t('mods.installed.resolvingShort')}
      delayMs={150}
    />
  {:else if hasRelation}
    <!-- The row's disclosure, one neutral control for both directions (spec D12): ⛓ own
         dependencies (by project; «?» while the platform could not describe them — unknown is not
         zero), ↑ dependents. Each figure keeps to its slot, so the list's figures line up (spec
         2026-09-30); the words live in its name and tooltip. Quiet at rest; `bg-muted` under the
         pointer, as a hovered row is `bg-subtle` already; accent only while its section is open.
         The transparent border is what forced-colors mode paints as its edge. -->
    <button
      type="button"
      data-testid="relation-pill"
      data-tour-ctx={tourAnchor ?? undefined}
      aria-expanded={expanded}
      aria-controls={expanded && root ? sectionId : undefined}
      aria-label={expandLabel}
      use:tooltip={{ text: relationTooltip, describe: !!depsUnknown }}
      class="w-full h-6 px-1 rounded border border-transparent inline-flex items-center gap-1.5 {expanded
        ? 'bg-accent-soft text-accent'
        : 'text-secondary hover:bg-muted'}"
      onclick={onToggleExpand}
    >
      <span class="relation-slot-dep inline-flex items-center gap-0.5" data-testid="relation-dep"
        >{#if figures.dep !== null}<Icon name="link" size={12} />{figures.dep}{/if}</span
      >
      <span class="relation-slot-by inline-flex items-center gap-0.5" data-testid="relation-by"
        >{#if figures.by !== null}<Icon name="arrowUp" size={12} />{figures.by}{/if}</span
      >
    </button>
  {/if}
{/snippet}

<!-- The second line is there only for a problem (spec D12): what used to sit here — the changelog
     chip and the dependency chip — is the update badge and the relation pill in the row now. It is
     the card's own second line (plan §5b V2), inside its surface and its accent strip. One reason,
     in full (it wraps rather than truncates: it is the reason, not a label), and the one fix its
     status chose. A warning keeps the longer compat sentence as its tooltip.
     Two columns lined up by baseline, so the icon stays on the reason's first line; the reason is
     text that wraps under itself, and «and N more · Fix…» follows it inline — one unit that never
     breaks — or takes one line of its own. As a row of flex items it broke between its parts: at
     820 px the icon stood alone on a line and the fix fell to a third (plan §5c, screenshot n01b). -->
{#snippet problemLine()}
  {#if problem}
    {@const tone = problem.level === 'blocking' ? 'text-danger' : 'text-warning-text'}
    <div
      class="flex items-baseline gap-2 text-xs"
      data-testid="row-problem"
      data-level={problem.level}
    >
      <span class="shrink-0"
        ><Icon
          name={problem.level === 'blocking' ? 'circleX' : 'warning'}
          size={14}
          class="inline-block align-middle {tone}"
        /></span
      >
      <p class="min-w-0 flex-1">
        <span class={tone} use:tooltip={problem.tooltip}>{problem.text}</span>
        {#if problem.more > 0 || problem.fix}
          <span
            class="ms-1 inline-flex items-center gap-x-2 whitespace-nowrap align-middle"
            data-testid="row-problem-actions"
          >
            {#if problem.more > 0}
              <button
                type="button"
                class="btn-link text-xs"
                data-testid="row-problem-more"
                onclick={() => onRevealProblems()}
              >
                {$t('mods.installed.reasonMore', { count: problem.more })}
              </button>
            {/if}
            {#if problem.fix}
              {@const fix = problem.fix}
              <button type="button" class="btn-secondary btn-xs" onclick={() => onProblemFix(fix)}>
                {fix.label}
              </button>
            {/if}
          </span>
        {/if}
      </p>
    </div>
  {/if}
{/snippet}

<div role="group" aria-label={installed.name}>
  <!-- The row «show in the list» scrolls into view and a removal moves focus into (`data-mod-row`): the card
       with its problem line, not the expanded DepSection below it. Pointing at it shows the
       card's own hover and nothing else — no other place this mod appears lights up. -->
  <div
    data-mod-row={rowKey}
    use:fieldFlash={{ active: flash, behavior: 'smooth', onDelivered: onFlashed }}
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
      relation={relationCell}
      below={problem ? problemLine : undefined}
      {onRevealFile}
      {onOpenProjectPage}
      {hold}
    />
  </div>
  <!-- A row whose platform details did not load still has its dependencies: the panel needs the
       graph's root, not the summary. -->
  {#if expanded && root}
    <DepSection
      id={sectionId}
      {root}
      {requiredBy}
      onInstall={onInstallDep}
      {onJump}
      onOpenDetail={onOpenDetailMod}
      {treeCtx}
      tourAnchors={tourAnchor === 'deps-cell'}
    />
  {/if}
</div>
