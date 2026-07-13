<script lang="ts" module>
  import type { IconName } from '$lib/ui/icons';
  export interface ContextMenuItem {
    label: string;
    icon?: IconName;
    danger?: boolean;
    disabled?: boolean;
    separatorBefore?: boolean;
    testId?: string;
    onSelect: () => void;
  }
</script>

<script lang="ts">
  import type { Snippet } from 'svelte';
  import Menu from '$lib/ui/Menu.svelte';

  // Reusable right-click / Shift+F10 menu. Wraps a target (children) in a
  // display:contents div so it captures the contextmenu + keyboard-open events
  // without affecting layout. The shared Menu renders the surface at the pointer
  // (or the focused element for keyboard open); this wrapper owns the trigger and
  // restores focus on close so keyboard users aren't dropped onto <body>.
  let {
    items,
    ariaLabel,
    children,
  }: { items: ContextMenuItem[]; ariaLabel: string; children: Snippet } = $props();

  const WIDTH = 220;
  const MARGIN = 8;
  const ROW = 34;

  let open = $state(false);
  let top = $state(0);
  let left = $state(0);
  let returnFocusEl: HTMLElement | null = null;

  function openAt(x: number, y: number) {
    // Remember where focus was so we can return it when the menu closes.
    returnFocusEl = document.activeElement as HTMLElement | null;
    left = Math.min(Math.max(x, MARGIN), Math.max(MARGIN, window.innerWidth - WIDTH - MARGIN));
    const estH = items.length * ROW + 10;
    top = Math.min(Math.max(y, MARGIN), Math.max(MARGIN, window.innerHeight - estH - MARGIN));
    open = true;
  }

  function onContextMenu(e: MouseEvent) {
    if (items.length === 0) return;
    e.preventDefault();
    e.stopPropagation();
    openAt(e.clientX, e.clientY);
  }

  function onTriggerKeydown(e: KeyboardEvent) {
    const isOpenKey = e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10');
    if (!isOpenKey || items.length === 0) return;
    e.preventDefault();
    const el = document.activeElement as HTMLElement | null;
    const r = el?.getBoundingClientRect();
    openAt(r ? r.left + 8 : MARGIN, r ? r.bottom : MARGIN);
  }

  function close() {
    open = false;
    // Restore focus to wherever it was when the menu opened (e.g. the card),
    // so keyboard users don't get dropped onto <body>.
    returnFocusEl?.focus?.();
    returnFocusEl = null;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="contents" oncontextmenu={onContextMenu} onkeydown={onTriggerKeydown}>
  {@render children()}
</div>

{#if open}
  <Menu {items} {ariaLabel} {top} {left} width={WIDTH} onClose={close} />
{/if}
