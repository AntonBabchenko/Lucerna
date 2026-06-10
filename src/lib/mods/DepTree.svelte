<script lang="ts">
  import type { DepTreeNode, ModSource } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import Self from './DepTree.svelte';

  let {
    nodes,
    hoveredKey,
    onHover,
    onInstall,
    onAdd,
    onJump = () => {},
    onOpenDetail,
  }: {
    nodes: DepTreeNode[];
    hoveredKey: string | null;
    onHover: (key: string | null) => void;
    onInstall: (node: DepTreeNode) => void;
    onAdd: (node: DepTreeNode) => void;
    // Jump to an installed dependency's own row in the list.
    onJump?: (node: DepTreeNode) => void;
    // Open the mod's info modal for any node (installed or not).
    onOpenDetail: (source: ModSource, projectId: string) => void;
  } = $props();

  const keyOf = (n: DepTreeNode) => `${n.source}:${n.project_id}`;
  const isInstalled = (n: DepTreeNode) =>
    n.status === 'satisfied' || n.status === 'optional_present';
</script>

<ul class="text-xs">
  {#each nodes as n (keyOf(n))}
    {@const k = keyOf(n)}
    <li>
      <div
        data-mod-key={k}
        class="flex items-center gap-2 py-0.5 px-1 rounded"
        class:bg-highlight={hoveredKey === k}
        role="treeitem"
        aria-selected={hoveredKey === k}
        tabindex="0"
        onmouseenter={() => onHover(k)}
        onmouseleave={() => onHover(null)}
        onfocus={() => onHover(k)}
        onblur={() => onHover(null)}
      >
        <!-- The name always opens the mod's info modal. For installed deps a
             separate ↗ button jumps to the mod's own row in the list. -->
        <button
          type="button"
          class="text-accent hover:underline text-left"
          onclick={() => onOpenDetail(n.source, n.project_id)}>{n.name}</button
        >
        {#if isInstalled(n)}
          <button
            type="button"
            class="text-accent inline-flex items-center justify-center"
            title={$t('mods.deps.jumpToTitle', { name: n.name })}
            aria-label={$t('mods.deps.jumpToTitle', { name: n.name })}
            onclick={() => onJump(n)}><Icon name="arrowUpRight" size={12} /></button
          >
        {/if}
        {#if n.status === 'satisfied' || n.status === 'optional_present'}
          <span class="inline-flex items-center gap-1 text-success"
            ><Icon name="success" size={12} />{$t('mods.deps.installedStatus')}</span
          >
        {:else if n.status === 'missing_required'}
          <span class="inline-flex items-center gap-1 text-danger"
            ><Icon name="circleX" size={12} />{$t('mods.deps.missingStatus')}</span
          >
          <button
            type="button"
            class="btn-primary btn-xs"
            aria-label={$t('mods.deps.installAriaLabel', { name: n.name })}
            onclick={() => onInstall(n)}>{$t('mods.card.install')}</button
          >
        {:else}
          <span class="text-muted italic">{$t('mods.deps.optionalStatus')}</span>
          <button
            type="button"
            class="btn-secondary btn-xs"
            aria-label={$t('mods.deps.addAriaLabel', { name: n.name })}
            onclick={() => onAdd(n)}>{$t('mods.deps.addBtn')}</button
          >
        {/if}
        {#if n.cycle}<span class="inline-flex items-center gap-1 text-placeholder"
            ><Icon name="refresh" size={12} />{$t('mods.deps.cycleStatus')}</span
          >{/if}
      </div>
      {#if n.children.length > 0 && !n.cycle}
        <div class="ml-4 border-l border-border-subtle pl-3">
          <Self
            nodes={n.children}
            {hoveredKey}
            {onHover}
            {onInstall}
            {onAdd}
            {onJump}
            {onOpenDetail}
          />
        </div>
      {/if}
    </li>
  {/each}
</ul>
