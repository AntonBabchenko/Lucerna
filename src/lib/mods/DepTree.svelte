<script lang="ts">
  import type { DepTreeNode, ModSource } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Self from './DepTree.svelte';

  let {
    nodes,
    outOfRangeKeys = new Set(),
    installingKeys = new Set(),
    hoveredKey,
    onHover,
    onInstall,
    onAdd,
    onJump = () => {},
    onOpenDetail,
  }: {
    nodes: DepTreeNode[];
    outOfRangeKeys?: Set<string>;
    // Keys (`source:project_id`) whose install is in flight — drives the
    // per-node BusyButton spinner. Empty in the common read-only render.
    installingKeys?: Set<string>;
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
          class="btn-tertiary text-left"
          onclick={() => onOpenDetail(n.source, n.project_id)}>{n.name}</button
        >
        {#if n.installed}
          <button
            type="button"
            class="text-accent inline-flex items-center justify-center"
            use:tooltip={$t('mods.deps.jumpToTitle', { name: n.name })}
            aria-label={$t('mods.deps.jumpToTitle', { name: n.name })}
            onclick={() => onJump(n)}><Icon name="arrowUpRight" size={12} /></button
          >
        {/if}
        {#if outOfRangeKeys.has(keyOf(n))}
          <span class="inline-flex items-center gap-1 text-danger"
            ><Icon name="circleX" size={12} />{$t('mods.preflight.treeOutOfRange')}</span
          >
        {:else if n.installed}
          <span class="inline-flex items-center gap-1 text-success"
            ><Icon name="success" size={12} />{$t('mods.deps.installedStatus')}</span
          >
        {:else}
          <!-- Absent, in the neutral register whatever the author declared. An
               absence the LOADER enforces is a pre-flight violation and is shown
               as such; the platform's word alone demands no attention here, so
               it gets no danger colour. The action is unchanged either way. -->
          {@const label =
            n.declared === 'required'
              ? $t('mods.deps.installAriaLabel', { name: n.name })
              : $t('mods.deps.addAriaLabel', { name: n.name })}
          <span class="text-secondary">{$t('mods.deps.notInstalledStatus')}</span>
          <span class="inline-flex" use:tooltip={label}>
            <BusyButton
              busy={installingKeys.has(keyOf(n))}
              class="btn-icon btn-icon-sm"
              aria-label={label}
              onclick={() => (n.declared === 'required' ? onInstall(n) : onAdd(n))}
            >
              <Icon name="download" size={12} />
            </BusyButton>
          </span>
        {/if}
        {#if n.cycle}<span class="inline-flex items-center gap-1 text-placeholder"
            ><Icon name="refresh" size={12} />{$t('mods.deps.cycleStatus')}</span
          >{/if}
      </div>
      {#if n.children.length > 0 && !n.cycle}
        <div class="ml-4 border-l border-border-subtle pl-3">
          <Self
            nodes={n.children}
            {outOfRangeKeys}
            {installingKeys}
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
