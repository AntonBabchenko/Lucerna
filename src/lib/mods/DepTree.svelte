<script lang="ts">
  import type { DepTreeNode, ModSource } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import Self from './DepTree.svelte';
  import { classifyDepNode, type DepTreeCtx, EMPTY_TREE_CTX } from './dep-node-state';

  // A WAI-ARIA tree (spec 2026-09-28 §6.3): `tree` / `group` / `treeitem`, one roving tab stop,
  // arrows / Home / End / Enter. Every level is one instance of this component; the root level
  // owns the state all of them share and handles the keys (they bubble up to it).
  type TreeState = {
    // Branches flipped from their default: the top level starts open, deeper ones closed.
    readonly toggled: SvelteSet<string>;
    // The item holding the tab stop (the one Tab enters the tree on).
    readonly stop: string | null;
    setActive: (path: string) => void;
    toggle: (path: string, wasOpen: boolean) => void;
  };

  let {
    nodes,
    depth = 0,
    parentPath = '',
    idPrefix = '',
    labelledby = undefined,
    dependentSha1 = null,
    ctx = EMPTY_TREE_CTX,
    tree = null,
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
    // Nesting bookkeeping, set by the level above (the root leaves them alone).
    depth?: number;
    parentPath?: string;
    idPrefix?: string;
    // The heading that names this tree (root only).
    labelledby?: string;
    // The enabled jar that declared THIS level's edges: the row's own mod at the top level, an
    // installed parent below it, null under an absent parent (it declared nothing we can judge).
    dependentSha1?: string | null;
    ctx?: DepTreeCtx;
    tree?: TreeState | null;
    outOfRangeKeys?: Set<string>;
    // Keys (`source:project_id`) whose install is in flight — drives the per-node BusyButton
    // spinner. Empty in the common read-only render.
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

  const uid = $props.id();
  const keyOf = (n: DepTreeNode) => `${n.source}:${n.project_id}`;
  const pathOf = (parent: string, n: DepTreeNode) => (parent ? `${parent}/${keyOf(n)}` : keyOf(n));
  const hasKids = (n: DepTreeNode) => n.children.length > 0 && !n.cycle;

  // The root level's state (a nested level makes one too, but uses the root's via `tree`).
  const toggled = new SvelteSet<string>();
  let activePath = $state<string | null>(null);
  const openAt = (path: string, level: number) => (level === 0) !== toggled.has(path);
  function visiblePaths(ns: DepTreeNode[], parent: string, level: number, out: string[]) {
    for (const n of ns) {
      const path = pathOf(parent, n);
      out.push(path);
      if (hasKids(n) && openAt(path, level)) visiblePaths(n.children, path, level + 1, out);
    }
    return out;
  }
  // The item the user last moved to — or, when it is gone (the graph is re-resolved after
  // every mod change) or hidden, the first item, so the tree can never lose its tab stop.
  const stopPath = $derived.by(() => {
    const visible = visiblePaths(nodes, '', 0, []);
    return activePath !== null && visible.includes(activePath) ? activePath : (visible[0] ?? null);
  });
  const own: TreeState = {
    toggled,
    get stop() {
      return stopPath;
    },
    setActive: (path) => {
      activePath = path;
    },
    // Membership means "flipped from the default", so one flip works at any depth.
    toggle: (path, wasOpen) => {
      if (toggled.has(path)) toggled.delete(path);
      else toggled.add(path);
      // A branch that closes around the tab stop takes it.
      if (wasOpen && activePath?.startsWith(`${path}/`)) activePath = path;
    },
  };
  const st = $derived(tree ?? own);
  const base = $derived(idPrefix || uid);
  const isOpen = (path: string) => (depth === 0) !== st.toggled.has(path);
  let rootEl = $state<HTMLUListElement | null>(null);

  // Focus that lands in the item at `path` itself (its row or its row's buttons) — not in a
  // nested item, whose focusin bubbles up through every item around it.
  const isOwnFocus = (e: FocusEvent, path: string) =>
    (e.target as HTMLElement).closest('[role="treeitem"]')?.getAttribute('data-path') === path;

  function onKeydown(e: KeyboardEvent) {
    const itemEl = e.target as HTMLElement;
    // Keys pressed on an item's own buttons stay theirs (Enter and Space activate them).
    if (!rootEl || itemEl.getAttribute('role') !== 'treeitem') return;
    const items = [...rootEl.querySelectorAll<HTMLElement>('[role="treeitem"]')];
    const i = items.indexOf(itemEl);
    const path = itemEl.dataset.path ?? '';
    const expanded = itemEl.getAttribute('aria-expanded');
    let next: HTMLElement | null = null;
    switch (e.key) {
      case 'ArrowDown':
        next = items[i + 1] ?? null;
        break;
      case 'ArrowUp':
        next = items[i - 1] ?? null;
        break;
      case 'Home':
        next = items[0] ?? null;
        break;
      case 'End':
        next = items[items.length - 1] ?? null;
        break;
      case 'ArrowRight':
        if (expanded === 'false') st.toggle(path, false);
        else if (expanded === 'true') next = items[i + 1] ?? null;
        break;
      case 'ArrowLeft':
        if (expanded === 'true') st.toggle(path, true);
        else next = itemEl.parentElement?.closest<HTMLElement>('[role="treeitem"]') ?? null;
        break;
      case 'Enter':
        // The item's own row (its first child), never a nested item's.
        itemEl.firstElementChild?.querySelector<HTMLButtonElement>('[data-tree-name]')?.click();
        break;
      default:
        return;
    }
    e.preventDefault();
    if (next) {
      st.setActive(next.dataset.path ?? '');
      next.focus();
    }
  }
</script>

<ul
  bind:this={rootEl}
  class={depth === 0 ? 'text-xs' : 'ml-4 border-l border-border-subtle pl-3'}
  role={depth === 0 ? 'tree' : 'group'}
  aria-labelledby={depth === 0 ? labelledby : undefined}
  onkeydown={depth === 0 ? onKeydown : undefined}
>
  {#each nodes as n, index (keyOf(n))}
    {@const k = keyOf(n)}
    {@const path = pathOf(parentPath, n)}
    {@const id = `${base}-${index}`}
    {@const open = hasKids(n) && isOpen(path)}
    {@const isStop = st.stop === path}
    {@const tab = isStop ? 0 : -1}
    {@const state = classifyDepNode({
      node: n,
      dependentSha1,
      report: ctx.report,
      projectOf: ctx.projectOf,
      outOfRange: outOfRangeKeys.has(k),
    })}
    <!-- Named by the mod, described by what the tree says about it: a name from content would
         also read every nested item. aria-selected follows the roving tab stop (single select,
         selection follows focus) — never hover (spec §6.3). -->
    <li
      role="treeitem"
      aria-level={depth + 1}
      aria-expanded={hasKids(n) ? open : undefined}
      aria-selected={isStop}
      aria-labelledby="{id}-name"
      aria-describedby={n.cycle ? `${id}-state ${id}-cycle` : `${id}-state`}
      tabindex={tab}
      data-path={path}
      data-node-state={state}
      onfocusin={(e) => {
        if (!isOwnFocus(e, path)) return;
        st.setActive(path);
        onHover(k);
      }}
      onfocusout={() => onHover(null)}
    >
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        data-mod-key={k}
        class="tree-row relative flex items-center gap-2 py-0.5 px-1 rounded"
        class:dep-highlight={hoveredKey === k}
        onmouseenter={() => onHover(k)}
        onmouseleave={() => onHover(null)}
      >
        {#if hasKids(n)}
          <!-- The mouse's way to open a branch; the keyboard's is ←/→ on the item. It never
               takes focus, so a click leaves the tab stop where it was. -->
          <button
            type="button"
            class="btn-icon btn-icon-sm"
            data-tree-toggle
            tabindex="-1"
            aria-hidden="true"
            onmousedown={(e) => e.preventDefault()}
            onclick={() => st.toggle(path, open)}
            ><Icon name={open ? 'chevronDown' : 'caret'} size={12} /></button
          >
        {:else}
          <span class="inline-block w-7 shrink-0" aria-hidden="true"></span>
        {/if}
        <!-- The name always opens the mod's info modal. For installed deps a separate ↗ button
             jumps to the mod's own row in the list. -->
        <button
          type="button"
          id="{id}-name"
          class="btn-tertiary text-left"
          data-tree-name
          tabindex={tab}
          onclick={() => onOpenDetail(n.source, n.project_id)}>{n.name}</button
        >
        {#if n.installed}
          <button
            type="button"
            class="text-accent inline-flex items-center justify-center"
            tabindex={tab}
            use:tooltip={$t('mods.deps.jumpToTitle', { name: n.name })}
            aria-label={$t('mods.deps.jumpToTitle', { name: n.name })}
            onclick={() => onJump(n)}><Icon name="arrowUpRight" size={12} /></button
          >
        {/if}
        {#if state === 'out_of_range'}
          <span id="{id}-state" class="inline-flex items-center gap-1 text-danger"
            ><Icon name="circleX" size={12} />{$t('mods.preflight.treeOutOfRange')}</span
          >
        {:else if state === 'installed'}
          <span id="{id}-state" class="inline-flex items-center gap-1 text-success"
            ><Icon name="success" size={12} />{$t('mods.deps.installedStatus')}</span
          >
        {:else if state === 'disabled'}
          <!-- Present but switched off: switching the jar back on, never a second copy. -->
          <span id="{id}-state" class="text-secondary">{$t('mods.deps.disabledStatus')}</span>
          <button
            type="button"
            class="btn-secondary btn-xs"
            tabindex={tab}
            aria-label={$t('mods.deps.enableAriaLabel', { name: n.name })}
            onclick={() => ctx.onEnable(n)}>{$t('mods.deps.enableBtn')}</button
          >
        {:else}
          <!-- Absent. Red only when the loader enforces it (a pre-flight violation of this very
               dependent names this project); the platform's word alone stays neutral. The action
               is the same either way. -->
          {@const label =
            n.declared === 'required'
              ? $t('mods.deps.installAriaLabel', { name: n.name })
              : $t('mods.deps.addAriaLabel', { name: n.name })}
          {#if state === 'loader_required'}
            <span id="{id}-state" class="inline-flex items-center gap-1 text-danger"
              ><Icon name="circleX" size={12} />{$t('mods.deps.stateLoaderRequired')}</span
            >
          {:else if state === 'platform_only'}
            <span id="{id}-state" class="text-secondary">{$t('mods.deps.statePlatformOnly')}</span>
          {:else if state === 'unknown'}
            <span id="{id}-state" class="text-secondary">{$t('mods.deps.stateUnknown')}</span>
          {:else}
            <span id="{id}-state" class="text-secondary">{$t('mods.deps.notInstalledStatus')}</span>
          {/if}
          <span class="inline-flex" use:tooltip={label}>
            <BusyButton
              busy={installingKeys.has(k)}
              class="btn-icon btn-icon-sm"
              aria-label={label}
              tabindex={tab}
              onclick={() => (n.declared === 'required' ? onInstall(n) : onAdd(n))}
            >
              <Icon name="download" size={12} />
            </BusyButton>
          </span>
        {/if}
        {#if n.cycle}<span id="{id}-cycle" class="inline-flex items-center gap-1 text-placeholder"
            ><Icon name="refresh" size={12} />{$t('mods.deps.cycleStatus')}</span
          >{/if}
      </div>
      {#if open}
        <Self
          nodes={n.children}
          depth={depth + 1}
          parentPath={path}
          idPrefix={id}
          tree={st}
          dependentSha1={n.installed ? ctx.enabledShaOf(k) : null}
          {ctx}
          {outOfRangeKeys}
          {installingKeys}
          {hoveredKey}
          {onHover}
          {onInstall}
          {onAdd}
          {onJump}
          {onOpenDetail}
        />
      {/if}
    </li>
  {/each}
</ul>
