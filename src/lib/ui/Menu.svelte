<script lang="ts">
  // Shared menu popover — the role="menu" surface, item list, keyboard nav, and
  // scroll/resize close that OverflowMenu (left-click ⋯) and ContextMenu
  // (right-click / Shift+F10) rendered byte-identically. The trigger wrappers own
  // opening, positioning (top/left/width), and focus-return via onClose; this
  // owns everything once the menu is positioned and mounted.
  import { onMount } from 'svelte';
  import { Icon } from '$lib/ui/icons';
  import type { ContextMenuItem } from '$lib/ui/cards/ContextMenu.svelte';

  let {
    items,
    ariaLabel,
    top,
    left,
    width,
    onClose,
  }: {
    items: ContextMenuItem[];
    ariaLabel: string;
    top: number;
    left: number;
    width: number;
    onClose: () => void;
  } = $props();

  // Menu mounts fresh on each open; onMount seeds the active row (first enabled
  // item, or -1 when every item is disabled).
  let activeIndex = $state(-1);
  let menuEl: HTMLDivElement | undefined = $state();

  const enabledIndexes = $derived(
    items.map((it, i) => (it.disabled ? -1 : i)).filter((i) => i >= 0),
  );

  function select(it: ContextMenuItem) {
    if (it.disabled) return;
    onClose();
    it.onSelect();
  }

  function onMenuKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onClose();
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

  // A fixed menu does not follow the trigger on layout shift — close on any
  // ancestor scroll (capture-phase, third arg true, so non-bubbling scroll from
  // a scrollable host is caught) or resize. Seed the active row and grab focus
  // on mount; the returned cleanup detaches the listeners on close/unmount.
  onMount(() => {
    activeIndex = items.findIndex((it) => !it.disabled);
    menuEl?.focus();
    const onScroll = () => onClose();
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('resize', onScroll);
    return () => {
      window.removeEventListener('scroll', onScroll, true);
      window.removeEventListener('resize', onScroll);
    };
  });
</script>

<!-- Click / right-click scrim closes the menu without triggering underlying UI. -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  role="presentation"
  class="fixed inset-0 z-40"
  onclick={onClose}
  oncontextmenu={(e) => {
    e.preventDefault();
    onClose();
  }}
></div>
<div
  bind:this={menuEl}
  role="menu"
  tabindex="-1"
  aria-label={ariaLabel}
  class="fixed z-50 max-h-[80vh] overflow-y-auto bg-surface border border-border-emphasis rounded shadow-md py-1 outline-none"
  style="top: {top}px; left: {left}px; width: {width}px;"
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
      data-testid={it.testId ?? undefined}
      class={`w-full flex items-center gap-2 px-3 py-1.5 text-sm text-left disabled:opacity-50 ${it.danger ? 'text-danger' : 'text-secondary'} ${activeIndex === i ? 'bg-subtle' : 'hover:bg-subtle'}`}
      onclick={() => select(it)}
      onmouseenter={() => (activeIndex = i)}
    >
      {#if it.icon}<Icon name={it.icon} size={15} />{/if}
      {it.label}
    </button>
  {/each}
</div>
