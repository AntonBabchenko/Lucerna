<script lang="ts">
  // Shared menu popover — the role="menu" surface, item list, keyboard nav, and
  // scroll/resize close that OverflowMenu (left-click ⋯) and ContextMenu
  // (right-click / Shift+F10) rendered byte-identically. The trigger wrappers own
  // opening, positioning (top/left/width), and focus-return via onClose; this
  // owns everything once the menu is positioned and mounted.
  //
  // `width` is a MINIMUM: the menu is as wide as its longest label (plan §5b V2 —
  // a fixed 230 px wrapped «Перепроверить совместимость и зависимости» over three
  // lines and squeezed its icon). A disabled item's reason wraps at the width the
  // labels set rather than setting it. A menu that grew keeps the edge it was
  // anchored by (`align`: the ⋯ menu hangs from its button's right edge, a
  // context menu from the pointer's left) and is clamped back on screen.
  import { onMount } from 'svelte';
  import { Icon } from '$lib/ui/icons';
  import { useLayer } from '$lib/ui/layer-stack.svelte';
  import { attachPopoverDismiss } from '$lib/ui/popover-dismiss';
  import type { ContextMenuItem } from '$lib/ui/menu-item';

  /** Distance kept from the window edge — the triggers' own clamp margin. */
  const MARGIN = 8;

  let {
    items,
    ariaLabel,
    top,
    left,
    width,
    align = 'start',
    onClose,
    openedByKeyboard = false,
  }: {
    items: ContextMenuItem[];
    ariaLabel: string;
    top: number;
    left: number;
    /** The minimum width; the menu grows to its longest label. */
    width: number;
    /** 'end': `left + width` is the edge the menu hangs from (it grows leftwards). */
    align?: 'start' | 'end';
    onClose: () => void;
    openedByKeyboard?: boolean;
  } = $props();

  // Menu mounts fresh on each open; onMount seeds the active row (first enabled
  // item for keyboard opens, -1 for pointer opens or when every item is disabled).
  let activeIndex = $state(-1);
  let menuEl: HTMLDivElement | undefined = $state();
  // Where a menu wider than `width` goes; null = where the trigger put it.
  let placedLeft = $state<number | null>(null);
  const surfaceStyle = $derived(
    `top: ${top}px; left: ${placedLeft ?? left}px; min-width: ${width}px; ` +
      `max-width: calc(100vw - ${2 * MARGIN}px);`,
  );

  // The measured width decides: an 'end' menu keeps its right edge, a 'start' one its left, and
  // either is clamped into the window. 0 = not laid out (no layout engine): leave it be.
  function place(actual: number): void {
    if (actual <= width) return;
    const wanted = align === 'end' ? left + width - actual : left;
    placedLeft = Math.min(
      Math.max(wanted, MARGIN),
      Math.max(MARGIN, window.innerWidth - actual - MARGIN),
    );
  }

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
      // Stop here so the layer router does not act on it too — Escape
      // dismisses only the menu, the way Select's dropdown does.
      e.stopPropagation();
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
  // Mounted only while open, so it is in the app's layer stack for its whole
  // life: a contextual tour underneath steps aside, and Escape from outside the
  // menu (focus elsewhere) still closes the menu first.
  useLayer(
    'popover',
    () => true,
    () => onClose(),
  );

  onMount(() => {
    activeIndex = openedByKeyboard ? items.findIndex((it) => !it.disabled) : -1;
    if (menuEl) place(menuEl.offsetWidth);
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
  class="fixed z-[var(--z-popover)] w-max max-h-[80vh] overflow-y-auto bg-surface border border-border-emphasis rounded shadow-md py-1 outline-none"
  style={surfaceStyle}
  onkeydown={onMenuKeydown}
>
  {#each items as it, i (it.label)}
    {#if it.separatorBefore}
      <div class="h-px bg-border-subtle my-1" aria-hidden="true"></div>
    {/if}
    {@const reason = it.disabled ? it.disabledReason : undefined}
    <button
      type="button"
      role="menuitem"
      tabindex="-1"
      disabled={it.disabled}
      data-testid={it.testId ?? undefined}
      class={`w-full flex gap-2 px-3 py-1.5 text-sm text-left ${reason ? 'items-start' : 'items-center disabled:opacity-50'} ${it.danger ? 'text-danger' : 'text-secondary'} ${activeIndex === i ? 'bg-subtle' : 'hover:bg-subtle'}`}
      onclick={() => select(it)}
      onmouseenter={() => (activeIndex = i)}
    >
      {#if it.icon}<Icon
          name={it.icon}
          size={15}
          class={reason ? 'mt-0.5 shrink-0 opacity-50' : 'shrink-0'}
        />{/if}
      {#if reason}
        <!-- The label dims like any disabled item; the reason stays at full
             contrast — halving the whole button would make it unreadable. The
             reason adds nothing to the menu's width (`w-0`) and fills the width
             the labels set (`min-w-full`), wrapping there. -->
        <span class="min-w-0 flex-1">
          <span class="block opacity-50">{it.label}</span>
          <span class="block w-0 min-w-full text-xs text-muted">{reason}</span>
        </span>
      {:else}
        {it.label}
      {/if}
    </button>
  {/each}
</div>
