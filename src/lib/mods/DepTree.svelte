<script lang="ts">
  import type { DepTreeNode } from '$lib/ipc/bindings';
  import Self from './DepTree.svelte';

  let {
    nodes,
    hoveredKey,
    onHover,
    onInstall,
    onAdd,
  }: {
    nodes: DepTreeNode[];
    hoveredKey: string | null;
    onHover: (key: string | null) => void;
    onInstall: (node: DepTreeNode) => void;
    onAdd: (node: DepTreeNode) => void;
  } = $props();

  const keyOf = (n: DepTreeNode) => `${n.source}:${n.project_id}`;
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
        <span class="text-primary">{n.name}</span>
        {#if n.status === 'satisfied' || n.status === 'optional_present'}
          <span class="text-success">✓ installed</span>
        {:else if n.status === 'missing_required'}
          <span class="text-danger">✕ missing</span>
          <button
            type="button"
            class="btn-primary btn-xs"
            aria-label={`Install ${n.name}`}
            onclick={() => onInstall(n)}>Install</button
          >
        {:else}
          <span class="text-muted italic">optional</span>
          <button
            type="button"
            class="btn-secondary btn-xs"
            aria-label={`Add ${n.name}`}
            onclick={() => onAdd(n)}>+ Add</button
          >
        {/if}
        {#if n.cycle}<span class="text-placeholder">↻ cycle</span>{/if}
      </div>
      {#if n.children.length > 0 && !n.cycle}
        <div class="ml-4 border-l border-border-subtle pl-3">
          <Self nodes={n.children} {hoveredKey} {onHover} {onInstall} {onAdd} />
        </div>
      {/if}
    </li>
  {/each}
</ul>
