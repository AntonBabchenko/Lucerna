<script lang="ts">
  import type { DepTreeNode, ModSource } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import Self from './DepTree.svelte';
  import {
    classifyDepNode,
    DEPS_UNKNOWN_KEY,
    type DepNodeState,
    type DepTreeCtx,
    EMPTY_TREE_CTX,
  } from './dep-node-state';

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
    rootKey = null,
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
    // What the tree says about each node — a version mismatch included — is read from the
    // report per edge (`classifyDepNode`): under this level's dependent only.
    ctx?: DepTreeCtx;
    tree?: TreeState | null;
    // The panel's own mod (`source:project_id`): the graph seeds every path with it, so an edge
    // back to it is a cycle the tree says as «this mod» (spec 2026-10-07 D5). Null = no panel.
    rootKey?: string | null;
    // Both name this level's `dependentSha1`: the install records the edge on the mod that
    // declared the node (spec §5.6) — none under an absent parent.
    onInstall: (node: DepTreeNode, dependentSha1: string | null) => void;
    onAdd: (node: DepTreeNode, dependentSha1: string | null) => void;
    // Jump to an installed dependency's own row in the list.
    onJump?: (node: DepTreeNode) => void;
    // Open the mod's info modal for any node (installed or not).
    onOpenDetail: (source: ModSource, projectId: string) => void;
  } = $props();

  const uid = $props.id();
  const keyOf = (n: DepTreeNode) => `${n.source}:${n.project_id}`;
  const pathOf = (parent: string, n: DepTreeNode) => (parent ? `${parent}/${keyOf(n)}` : keyOf(n));
  const hasKids = (n: DepTreeNode) => n.children.length > 0 && !n.cycle;
  // An installed node without children may have them unknown (the platform could not describe
  // its installed version): marked, so it never reads as needing nothing.
  const depsUnknownOf = (n: DepTreeNode) => (n.installed ? (n.deps_unknown ?? null) : null);
  // The top level sits under a heading that says required or optional; nothing heads a deeper
  // level, which mixes what its parent requires with what it only offers. So a node there that
  // its parent declares optional says so itself — or it reads as required and missing.
  const optionalBelowTop = (n: DepTreeNode) => depth > 0 && n.declared === 'optional';
  // A node with a row in the list — installed, or there but switched off: it can be switched,
  // removed and shown; an absent one can only be installed.
  const hasRow = (n: DepTreeNode) => n.installed || !!n.disabled;
  // The panel's own mod met again below. Plainly installed, «this mod» IS its state (the green
  // «installed» would say nothing about the mod the panel hangs under); in any other state — a
  // version mismatch on that edge — it is a mark after the state, like any other repeat.
  const isSelf = (n: DepTreeNode) => n.cycle && rootKey !== null && keyOf(n) === rootKey;
  const selfAsState = (n: DepTreeNode, state: DepNodeState) => isSelf(n) && state === 'installed';
  // What the tree says about an item: optional (below the top), its state, then any marks after
  // it. Only ids in the DOM: «this mod» as the state has no mark of its own.
  const describedBy = (id: string, n: DepTreeNode, selfState: boolean) =>
    [
      optionalBelowTop(n) ? `${id}-optional` : '',
      `${id}-state`,
      depsUnknownOf(n) ? `${id}-unknown` : '',
      n.cycle && !selfState ? `${id}-cycle` : '',
    ]
      .filter(Boolean)
      .join(' ');

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

  // The row is the pointer's way to the mod's details, as a list row is (spec 2026-10-07 D1); the
  // name button stays the keyboard's (Enter). The row's own buttons act for themselves — the name
  // too, whose click bubbles here — and a drag that selected text is no click on the row.
  function onRowClick(e: MouseEvent, n: DepTreeNode) {
    if ((e.target as Element).closest('button')) return;
    if ((window.getSelection()?.toString() ?? '') !== '') return;
    onOpenDetail(n.source, n.project_id);
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
    {@const unknownWhy = depsUnknownOf(n)}
    {@const state = classifyDepNode({
      node: n,
      dependentSha1,
      report: ctx.report,
      projectOf: ctx.projectOf,
    })}
    {@const selfState = selfAsState(n, state)}
    {@const present = hasRow(n)}
    {@const busy = !present && ctx.installing(k)}
    {@const toggleLabel = n.installed
      ? $t('mods.deps.disableAriaLabel', { name: n.name })
      : $t('mods.deps.enableAriaLabel', { name: n.name })}
    {@const presenceLabel = present
      ? $t('mods.deps.uninstallAriaLabel', { name: n.name })
      : n.declared === 'required'
        ? $t('mods.deps.installAriaLabel', { name: n.name })
        : $t('mods.deps.addAriaLabel', { name: n.name })}
    {@const jumpLabel = $t('mods.deps.jumpToTitle', { name: n.name })}
    <!-- Named by the mod, described by what the tree says about it: a name from content would
         also read every nested item. aria-selected follows the roving tab stop (single select,
         selection follows focus) — never hover (spec §6.3). -->
    <li
      role="treeitem"
      aria-level={depth + 1}
      aria-expanded={hasKids(n) ? open : undefined}
      aria-selected={isStop}
      aria-labelledby="{id}-name"
      aria-describedby={describedBy(id, n, selfState)}
      tabindex={tab}
      data-path={path}
      data-node-state={state}
      onfocusin={(e) => {
        if (isOwnFocus(e, path)) st.setActive(path);
      }}
    >
      <!-- A row reads like an Installed row (spec 2026-10-07): the list's hover fill, a click
           anywhere on it opens the mod, and on the right its state, then the list's own icon
           actions in three columns that line up at every depth — the nested group indents the
           left edge only. Every row is one height while it fits on one line (`min-h-7`: the
           chevron's and a button's); short of room the name is cut first, then the right group
           takes a line of its own. -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions — the
           pointer's shortcut to the name button, which is the keyboard path (Enter on the item). -->
      <div
        class="tree-row group flex min-h-7 cursor-pointer flex-wrap items-center gap-x-2 rounded px-1 transition-colors hover:bg-subtle"
        onclick={(e) => onRowClick(e, n)}
      >
        <span class="flex min-w-0 grow basis-[12ch] items-center gap-2">
          {#if hasKids(n)}
            <!-- The mouse's way to open a branch; the keyboard's is ←/→ on the item. It never
                 takes focus, so a click leaves the tab stop where it was. -->
            <button
              type="button"
              class="btn-icon btn-icon-sm shrink-0"
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
          <!-- The name opens the mod's info modal, as the row does; it reads as the row's text,
               not as a link. Cut short, it shows itself whole in a tooltip. -->
          <button
            type="button"
            id="{id}-name"
            class="min-w-0 truncate rounded text-left text-secondary transition-colors group-hover:text-primary focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent"
            data-tree-name
            tabindex={tab}
            use:tooltip={{ text: n.name, whenOverflowing: true, describe: false }}
            onclick={() => onOpenDetail(n.source, n.project_id)}>{n.name}</button
          >
          {#if optionalBelowTop(n)}
            <span id="{id}-optional" class="shrink-0 text-secondary"
              >{$t('mods.deps.optionalMark')}</span
            >
          {/if}
        </span>
        <span class="ms-auto flex shrink-0 items-center">
          <span class="me-1 inline-flex items-center gap-1 whitespace-nowrap" data-state-group>
            {#if selfState}
              <span
                id="{id}-state"
                class="inline-flex items-center gap-1 text-placeholder"
                use:tooltip={{
                  text: $t('mods.deps.cycleSelfTooltip', { name: n.name }),
                  describe: false,
                }}><Icon name="seeAbove" size={12} />{$t('mods.deps.cycleSelf')}</span
              >
            {:else if state === 'out_of_range'}
              <span id="{id}-state" class="inline-flex items-center gap-1 text-danger"
                ><Icon name="circleX" size={12} />{$t('mods.preflight.treeOutOfRange')}</span
              >
            {:else if state === 'installed'}
              <span id="{id}-state" class="inline-flex items-center gap-1 text-success"
                ><Icon name="success" size={12} />{$t('mods.deps.installedStatus')}</span
              >
            {:else if state === 'disabled'}
              <span id="{id}-state" class="text-secondary">{$t('mods.deps.disabledStatus')}</span>
            {:else if state === 'loader_required'}
              <!-- Red only when the loader enforces it (a pre-flight violation of this very
                   dependent names this project); the platform's word alone stays neutral. -->
              <span id="{id}-state" class="inline-flex items-center gap-1 text-danger"
                ><Icon name="circleX" size={12} />{$t('mods.deps.stateLoaderRequired')}</span
              >
            {:else if state === 'platform_only'}
              <span id="{id}-state" class="text-secondary">{$t('mods.deps.statePlatformOnly')}</span
              >
            {:else if state === 'unknown'}
              <span id="{id}-state" class="text-secondary">{$t('mods.deps.stateUnknown')}</span>
            {:else}
              <span id="{id}-state" class="text-secondary"
                >{$t('mods.deps.notInstalledStatus')}</span
              >
            {/if}
            {#if unknownWhy}
              <!-- Set apart from the state before it: «installed · dependencies unknown». -->
              <span class="text-placeholder" aria-hidden="true">·</span>
              <span
                id="{id}-unknown"
                class="text-secondary"
                use:tooltip={$t(DEPS_UNKNOWN_KEY[unknownWhy])}
                >{$t('mods.deps.depsUnknownStatus')}</span
              >
            {/if}
            {#if n.cycle && !selfState}
              <!-- Shown higher in this branch, so not expanded again: the panel's own mod, or
                   another one above. -->
              <span class="text-placeholder" aria-hidden="true">·</span>
              <span
                id="{id}-cycle"
                class="inline-flex items-center gap-1 text-placeholder"
                use:tooltip={{
                  text: isSelf(n)
                    ? $t('mods.deps.cycleSelfTooltip', { name: n.name })
                    : $t('mods.deps.cycleAboveTooltip', { name: n.name }),
                  describe: false,
                }}
                ><Icon name="seeAbove" size={12} />{isSelf(n)
                  ? $t('mods.deps.cycleSelf')
                  : $t('mods.deps.cycleAbove')}</span
              >
            {/if}
            {#if state === 'out_of_range'}
              {@const conflict = ctx.conflictOf(n, dependentSha1)}
              {#if conflict}
                <!-- The planner's «Fix…», as on the panel row and the mod's own line: its offers
                     show in «What stops the game». Beside the state it remedies, a wrench. -->
                <button
                  type="button"
                  class="btn-icon btn-icon-sm"
                  tabindex={tab}
                  aria-label={$t('mods.deps.fixConflictAriaLabel', { name: n.name })}
                  use:tooltip={$t('mods.deps.fixConflictAriaLabel', { name: n.name })}
                  onclick={() => ctx.onPlan(conflict)}><Icon name="wrench" size={15} /></button
                >
              {/if}
            {/if}
          </span>
          <!-- The Installed row's own actions (ModCard): switch, install or remove, show in the
               list. Each column holds ONE button whatever the node's state, so the button the
               user pressed is still there — and still focused — when the graph comes back with
               the node switched off or absent. Where it cannot act it stays, inactive, with its
               reason on the wrapper (a disabled button fires no pointer events). -->
          <span
            class="inline-flex w-7 shrink-0 justify-center"
            data-slot="toggle"
            use:tooltip={present
              ? null
              : { text: $t('mods.deps.toggleUnavailable', { name: n.name }), describe: false }}
          >
            <button
              type="button"
              class="btn-icon btn-icon-sm {n.installed ? 'btn-icon-success' : '!text-muted'}"
              disabled={!present}
              tabindex={tab}
              aria-label={toggleLabel}
              use:tooltip={present ? toggleLabel : null}
              onclick={() => (n.installed ? ctx.onDisable(n) : ctx.onEnable(n))}
              ><Icon name="power" size={15} /></button
            >
          </span>
          <span class="inline-flex w-7 shrink-0 justify-center" data-slot="presence">
            <!-- Busy is aria-disabled, not disabled: a disabled button drops focus, and the node
                 turns installed under it when the install lands. -->
            <button
              type="button"
              class="btn-icon btn-icon-sm {present ? 'btn-icon-danger' : '!text-accent'}"
              tabindex={tab}
              aria-label={presenceLabel}
              aria-busy={busy ? 'true' : undefined}
              aria-disabled={busy ? 'true' : undefined}
              use:tooltip={busy ? null : presenceLabel}
              onclick={() => {
                if (present) ctx.onUninstall(n);
                else if (busy) return;
                else if (n.declared === 'required') onInstall(n, dependentSha1);
                else onAdd(n, dependentSha1);
              }}
            >
              {#if busy}
                <Spinner size="sm" />
              {:else}
                <Icon name={present ? 'trash' : 'download'} size={15} />
              {/if}
            </button>
          </span>
          <span
            class="inline-flex w-7 shrink-0 justify-center"
            data-slot="locate"
            use:tooltip={present
              ? null
              : { text: $t('mods.deps.jumpUnavailable', { name: n.name }), describe: false }}
          >
            <button
              type="button"
              class="btn-icon btn-icon-sm"
              disabled={!present}
              tabindex={tab}
              aria-label={jumpLabel}
              use:tooltip={present ? jumpLabel : null}
              onclick={() => onJump(n)}><Icon name="locate" size={15} /></button
            >
          </span>
        </span>
      </div>
      {#if open}
        <Self
          nodes={n.children}
          depth={depth + 1}
          parentPath={path}
          idPrefix={id}
          tree={st}
          {rootKey}
          dependentSha1={n.installed ? ctx.enabledShaOf(k) : null}
          {ctx}
          {onInstall}
          {onAdd}
          {onJump}
          {onOpenDetail}
        />
      {/if}
    </li>
  {/each}
</ul>
