<script lang="ts" module>
  import type { IconName } from '$lib/ui/icons';
  export interface ContextMenuItem {
    label: string;
    icon?: IconName;
    danger?: boolean;
    disabled?: boolean;
    separatorBefore?: boolean;
    onSelect: () => void;
  }
</script>

<script lang="ts">
  import { Icon } from '$lib/ui/icons';
  import type { Snippet } from 'svelte';

  // Reusable right-click / Shift+F10 menu. Wraps a target (children) in a
  // display:contents div so it captures the contextmenu + keyboard-open events
  // without affecting layout. The menu itself is position:fixed at the pointer
  // (or the focused element for keyboard open), mirroring HelpPopover's approach
  // for escaping a host's overflow box. The global +layout guard already blocks
  // the native menu, so we only open our own.
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
  let activeIndex = $state(-1);
  let menuEl: HTMLDivElement | undefined = $state();

  const enabledIndexes = $derived(
    items.map((it, i) => (it.disabled ? -1 : i)).filter((i) => i >= 0),
  );

  function openAt(x: number, y: number) {
    left = Math.min(Math.max(x, MARGIN), Math.max(MARGIN, window.innerWidth - WIDTH - MARGIN));
    const estH = items.length * ROW + 10;
    top = Math.min(Math.max(y, MARGIN), Math.max(MARGIN, window.innerHeight - estH - MARGIN));
    open = true;
    activeIndex = enabledIndexes[0] ?? -1;
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
    activeIndex = -1;
  }

  function select(it: ContextMenuItem) {
    if (it.disabled) return;
    close();
    it.onSelect();
  }

  function onMenuKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (enabledIndexes.length === 0) return;
      const pos = Math.max(0, enabledIndexes.indexOf(activeIndex));
      const next =
        e.key === 'ArrowDown'
          ? (pos + 1) % enabledIndexes.length
          : (pos - 1 + enabledIndexes.length) % enabledIndexes.length;
      activeIndex = enabledIndexes[next];
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      const it = items[activeIndex];
      if (it) select(it);
    }
  }

  $effect(() => {
    if (!open) return;
    menuEl?.focus();
    const onScroll = () => close();
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('resize', onScroll);
    return () => {
      window.removeEventListener('scroll', onScroll, true);
      window.removeEventListener('resize', onScroll);
    };
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="contents" oncontextmenu={onContextMenu} onkeydown={onTriggerKeydown}>
  {@render children()}
</div>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    role="presentation"
    class="fixed inset-0 z-40"
    onclick={close}
    oncontextmenu={(e) => {
      e.preventDefault();
      close();
    }}
  ></div>
  <div
    bind:this={menuEl}
    role="menu"
    tabindex="-1"
    aria-label={ariaLabel}
    class="fixed z-50 bg-surface border border-border-emphasis rounded shadow-md py-1 outline-none"
    style="top: {top}px; left: {left}px; width: {WIDTH}px;"
    onkeydown={onMenuKeydown}
  >
    {#each items as it, i (it.label)}
      {#if it.separatorBefore}
        <div class="h-px bg-border-subtle my-1" aria-hidden="true"></div>
      {/if}
      <button
        type="button"
        role="menuitem"
        tabindex="-1"
        disabled={it.disabled}
        class={`w-full flex items-center gap-2 px-3 py-1.5 text-sm text-left disabled:opacity-50 ${it.danger ? 'text-danger' : 'text-secondary'} ${activeIndex === i ? 'bg-subtle' : 'hover:bg-subtle'}`}
        onclick={() => select(it)}
        onmouseenter={() => (activeIndex = i)}
      >
        {#if it.icon}<Icon name={it.icon} size={15} />{/if}
        {it.label}
      </button>
    {/each}
  </div>
{/if}
