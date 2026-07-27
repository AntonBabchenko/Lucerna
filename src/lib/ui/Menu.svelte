<script lang="ts">
  // Shared menu popover — the role="menu" surface, item list, keyboard nav, and
  // scroll/resize close that OverflowMenu (left-click ⋯) and ContextMenu
  // (right-click / Shift+F10) rendered byte-identically. The trigger wrappers own
  // opening, positioning (top/left/width), and focus-return via onClose; this
  // owns everything once the menu is positioned and mounted.
  import { onMount } from 'svelte';
  import { Icon } from '$lib/ui/icons';
  import { attachPopoverDismiss } from '$lib/ui/popover-dismiss';
  import type { ContextMenuItem } from '$lib/ui/cards/ContextMenu.svelte';

  let {
    items,
    ariaLabel,
    top,
    left,
    width,
    onClose,
    openedByKeyboard = false,
  }: {
    items: ContextMenuItem[];
    ariaLabel: string;
    top: number;
    left: number;
    width: number;
    onClose: () => void;
    openedByKeyboard?: boolean;
  } = $props();

  // Menu mounts fresh on each open; onMount seeds the active row (first enabled
  // item for keyboard opens, -1 for pointer opens or when every item is disabled).
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
      const pos = enabledIndexes.indexOf(activeIndex);
      let next: number;
      if (pos === -1) {
        // No active item yet (pointer open): ArrowDown enters at the top,
        // ArrowUp enters at the bottom.
        next = e.key === 'ArrowDown' ? 0 : enabledIndexes.length - 1;
      } else {
        next =
          e.key === 'ArrowDown'
            ? (pos + 1) % enabledIndexes.length
            : (pos - 1 + enabledIndexes.length) % enabledIndexes.length;
      }
      activeIndex = enabledIndexes[next];
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      const it = items[activeIndex];
      if (it) select(it);
    }
  }

  // OS menu convention: pointer-open highlights nothing until hover/arrows;
  // only keyboard-open pre-highlights the first enabled item. Grab focus on
  // mount, then close on any ancestor scroll / resize via the shared helper.
  // ignoreScrollWithin keeps a tall internally-scrolling menu (max-h-[80vh])
  // from dismissing itself when the user wheels it — a fix the hand-rolled menu
  // listeners lacked. The returned cleanup detaches the listeners on close/unmount.
  onMount(() => {
    activeIndex = openedByKeyboard ? items.findIndex((it) => !it.disabled) : -1;
    menuEl?.focus();
    return attachPopoverDismiss({ onDismiss: onClose, ignoreScrollWithin: () => menuEl });
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
  class="fixed z-[var(--z-popover)] max-h-[80vh] overflow-y-auto bg-surface border border-border-emphasis rounded shadow-md py-1 outline-none"
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
