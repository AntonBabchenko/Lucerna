<script lang="ts">
  import type { DepTreeNode } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import Self from './DepTree.svelte';

  let {
    nodes,
    hoveredKey,
    onHover,
    onInstall,
    onAdd,
    onJump = () => {},
  }: {
    nodes: DepTreeNode[];
    hoveredKey: string | null;
    onHover: (key: string | null) => void;
    onInstall: (node: DepTreeNode) => void;
    onAdd: (node: DepTreeNode) => void;
    // Jump to an installed dependency's own row in the list.
    onJump?: (node: DepTreeNode) => void;
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
        {#if isInstalled(n)}
          <!-- Installed dep → click the name to jump to its row in the list. -->
          <button
            type="button"
            class="text-accent hover:underline text-left"
            title={$t('mods.deps.jumpToTitle', { name: n.name })}
            onclick={() => onJump(n)}>{n.name} ↗</button
          >
        {:else}
          <span class="text-primary">{n.name}</span>
        {/if}
        {#if n.status === 'satisfied' || n.status === 'optional_present'}
          <span class="text-success">{$t('mods.deps.installedStatus')}</span>
        {:else if n.status === 'missing_required'}
          <span class="text-danger">{$t('mods.deps.missingStatus')}</span>
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
        {#if n.cycle}<span class="text-placeholder">{$t('mods.deps.cycleStatus')}</span>{/if}
      </div>
      {#if n.children.length > 0 && !n.cycle}
        <div class="ml-4 border-l border-border-subtle pl-3">
          <Self nodes={n.children} {hoveredKey} {onHover} {onInstall} {onAdd} {onJump} />
        </div>
      {/if}
    </li>
  {/each}
</ul>
