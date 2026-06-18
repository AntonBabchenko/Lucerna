<script lang="ts">
  import type { DepRoot, DepTreeNode, ModSource } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import DepTree from '../DepTree.svelte';
  import type { RequiredByEntry } from './dep-graph.svelte';

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
</script>

<!-- onAdd and onInstall both resolve to the same install handler here: in this
     view, "install missing required" and "add recommended" trigger the identical
     resolve-and-install path. DepTree keeps them separate for other callers. -->
<!-- Inset, bordered, and gapped below so the expanded tree reads as nested
     content under its mod and is clearly separated from the next mod row
     (a full-width grey block blended into the following row). -->
<div class="mx-3 mb-2 rounded-md border border-border-subtle bg-subtle/40 px-3 py-2">
  {#if root.required.length > 0}
    <div class="text-[10px] uppercase tracking-wide text-muted mt-1">
      {$t('mods.installed.sectionRequires')}
    </div>
    <DepTree
      nodes={root.required}
      {outOfRangeKeys}
      {hoveredKey}
      {onHover}
      {onInstall}
      onAdd={onInstall}
      {onJump}
      {onOpenDetail}
    />
  {/if}
  {#if root.optional.length > 0}
    <div class="text-[10px] uppercase tracking-wide text-muted mt-2">
      {$t('mods.installed.sectionRecommended')}
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
