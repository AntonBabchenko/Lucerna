<script lang="ts">
  import type { DepRoot, DepTreeNode } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import DepTree from '../DepTree.svelte';

  let {
    root,
    requiredByNames,
    hoveredKey,
    onHover,
    onInstall,
    onJump,
  }: {
    root: DepRoot;
    requiredByNames: string[];
    hoveredKey: string | null;
    onHover: (k: string | null) => void;
    onInstall: (node: DepTreeNode) => void;
    onJump: (node: DepTreeNode) => void;
  } = $props();
</script>

<div class="px-4 pb-3 bg-subtle/40">
  {#if root.required.length > 0}
    <div class="text-[10px] uppercase tracking-wide text-muted mt-1">
      {$t('mods.installed.sectionRequires')}
    </div>
    <DepTree nodes={root.required} {hoveredKey} {onHover} {onInstall} onAdd={onInstall} {onJump} />
  {/if}
  {#if root.optional.length > 0}
    <div class="text-[10px] uppercase tracking-wide text-muted mt-2">
      {$t('mods.installed.sectionRecommended')}
    </div>
    <DepTree nodes={root.optional} {hoveredKey} {onHover} {onInstall} onAdd={onInstall} {onJump} />
  {/if}
  {#if requiredByNames.length > 0}
    <div class="text-[10px] uppercase tracking-wide text-muted mt-2">
      {$t('mods.installed.sectionRequiredBy')}
    </div>
    <div class="text-xs text-secondary">{requiredByNames.join(', ')}</div>
  {/if}
</div>
