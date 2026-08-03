<script lang="ts">
  import type { DepRoot, DepTreeNode, ModSource } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import DepTree from '../DepTree.svelte';
  import type { RequiredByEntry } from './dep-graph.svelte';
  import { dismissClaim, isClaimDismissed, restoreClaim } from '$lib/mods/dep-claim-dismiss';

  let {
    root,
    requiredBy,
    hoveredKey,
    onHover,
    onInstall,
    onJump,
    onOpenDetail,
    outOfRangeKeys = new Set(),
  }: {
    root: DepRoot;
    requiredBy: RequiredByEntry[];
    hoveredKey: string | null;
    onHover: (k: string | null) => void;
    onInstall: (node: DepTreeNode) => void;
    onJump: (target: { source: ModSource; project_id: string }) => void;
    onOpenDetail: (source: ModSource, projectId: string) => void;
    outOfRangeKeys?: Set<string>;
  } = $props();

  // Owner of the claims rendered here. Dismissal is keyed on the (mod, dep)
  // PAIR, so the acknowledgement travels with the mod rather than the instance.
  const owner = $derived({ source: root.source, project_id: root.project_id });
  const refOf = (n: DepTreeNode) => ({ source: n.source, project_id: n.project_id });

  // An INSTALLED dependency is never hidden, even if its claim was settled while
  // it was absent — the tree's job is still to show the relationship.
  const isHidden = (n: DepTreeNode) =>
    !n.installed && n.declared === 'required' && isClaimDismissed(owner, refOf(n));
  const visibleRequired = $derived(root.required.filter((n) => !isHidden(n)));
  const hidden = $derived(root.required.filter(isHidden));
</script>

<!-- onAdd and onInstall both resolve to the same install handler here: in this
     view, "install missing required" and "add recommended" trigger the identical
     resolve-and-install path. DepTree keeps them separate for other callers. -->
<!-- Inset, bordered, and gapped below so the expanded tree reads as nested
     content under its mod and is clearly separated from the next mod row
     (a full-width grey block blended into the following row). -->
<div class="mx-3 mb-2 rounded-md border border-border-subtle bg-subtle/40 px-3 py-2">
  <!-- The headings attribute rather than assert. "Requires" was the launcher
       speaking; the platform's dependency list is the author speaking, and a
       measured mod's list is contradicted by its own jar descriptor. -->
  {#if visibleRequired.length > 0}
    <div class="text-[10px] uppercase tracking-wide text-muted mt-1">
      {$t('mods.installed.sectionAuthorRequired')}
    </div>
    <DepTree
      nodes={visibleRequired}
      {outOfRangeKeys}
      {hoveredKey}
      {onHover}
      {onInstall}
      onAdd={onInstall}
      {onJump}
      {onOpenDetail}
      onDismissClaim={(n) => dismissClaim(owner, refOf(n))}
    />
  {/if}
  {#if hidden.length > 0}
    <!-- A muted line, not the amber DiagnosisRestoreButton: that is the
         vocabulary of a warning, which is the tone being removed here. The
         expand chip always renders when a mod has any relationship, so this
         path back can never be lost. -->
    <button
      type="button"
      class="btn-tertiary text-xs text-muted mt-2"
      data-testid="claim-restore"
      onclick={() => hidden.forEach((n) => restoreClaim(owner, refOf(n)))}
    >
      {$t('mods.deps.claimsHidden', { count: hidden.length })}
    </button>
  {/if}
  {#if root.optional.length > 0}
    <div class="text-[10px] uppercase tracking-wide text-muted mt-2">
      {$t('mods.installed.sectionAuthorOptional')}
    </div>
    <DepTree
      nodes={root.optional}
      {outOfRangeKeys}
      {hoveredKey}
      {onHover}
      {onInstall}
      onAdd={onInstall}
      {onJump}
      {onOpenDetail}
    />
  {/if}
  {#if requiredBy.length > 0}
    <div class="text-[10px] uppercase tracking-wide text-muted mt-2">
      {$t('mods.installed.sectionRequiredBy')}
    </div>
    <div class="flex flex-wrap gap-x-3 gap-y-0.5 text-xs">
      {#each requiredBy as e (e.sha1)}
        {@const k = `${e.source}:${e.projectId}`}
        <!-- Name opens the mod's info modal; the separate ↗ jumps to the
             requiring mod's own row — mirroring the dependency tree. The keyed
             wrapper carries the cross-highlight so hovering either control marks
             the requiring mod's row. -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <span
          data-mod-key={k}
          class="inline-flex items-center gap-1 rounded px-1 -mx-1"
          class:bg-highlight={hoveredKey === k}
          onmouseenter={() => onHover(k)}
          onmouseleave={() => onHover(null)}
        >
          <button
            type="button"
            class="btn-tertiary"
            onclick={() => onOpenDetail(e.source, e.projectId)}>{e.name}</button
          >
          <button
            type="button"
            class="text-accent inline-flex items-center justify-center"
            use:tooltip={$t('mods.deps.jumpToTitle', { name: e.name })}
            aria-label={$t('mods.deps.jumpToTitle', { name: e.name })}
            onclick={() => onJump({ source: e.source, project_id: e.projectId })}
            ><Icon name="arrowUpRight" size={12} /></button
          >
        </span>
      {/each}
    </div>
  {/if}
</div>
