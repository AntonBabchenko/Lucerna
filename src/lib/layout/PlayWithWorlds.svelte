<script lang="ts">
  import { Icon } from '$lib/ui/icons';
  import type { WorldQuickEntry } from '$lib/ipc/bindings';

  // The green Play button plus a hover/keyboard popover of the instance's
  // worlds. Single-click launches normally (onPlay); the popover lets the
  // user jump straight into a world. The popover only exists when
  // `menuEnabled` (Quick Play supported + instance ready + not running,
  // decided by the page) AND there is at least one world. position:fixed so it
  // escapes the sidebar's overflow box; it opens downward, attached flush to the
  // button as one block. Mirrors OverflowMenu's close-on-scroll/resize.
  let {
    worlds,
    onPlay,
    onQuickPlayWorld,
    menuEnabled,
    label,
    menuLabel,
  }: {
    worlds: WorldQuickEntry[];
    onPlay: () => void;
    onQuickPlayWorld: (folderName: string) => void;
    menuEnabled: boolean;
    label: string;
    // Accessible name for the popup itself. Should describe the choices
    // ("launch into a world"), not echo the button verb. Falls back to `label`.
    menuLabel?: string;
  } = $props();

  const HOVER_DELAY_MS = 200;
  const MARGIN = 8;

  let open = $state(false);
  let top = $state(0);
  let left = $state(0);
  let width = $state(0);
  let maxHeight = $state(0);
  let hoverTimer: ReturnType<typeof setTimeout> | null = null;
  let triggerEl = $state<HTMLButtonElement>();
  let menuEl = $state<HTMLDivElement>();
  // Slots are set to null by Svelte when a row unmounts; type honestly so the
  // `?.` guards below are not a lie (mirrors TabBar.svelte's bind:this array).
  let itemEls = $state<(HTMLButtonElement | null)[]>([]);

  const canOpen = $derived(menuEnabled && worlds.length > 0);

  function place() {
    const r = triggerEl?.getBoundingClientRect();
    if (!r) return;
    // Match the trigger's width and anchor the menu's top flush to the trigger's
    // bottom → opens downward as one attached block. Flush (no gap) so moving the
    // cursor down into the menu never crosses dead space outside this wrapper,
    // which would fire mouseleave and close it before the pointer lands.
    width = r.width;
    left = r.left;
    top = r.bottom;
    // Cap to the space below so a long list scrolls instead of running off-window.
    maxHeight = Math.max(0, window.innerHeight - top - MARGIN);
  }

  function openMenu(focusFirst: boolean) {
    if (!canOpen) return;
    place();
    open = true;
    if (focusFirst) queueMicrotask(() => itemEls[0]?.focus());
  }

  function cancelHoverTimer() {
    if (hoverTimer !== null) {
      clearTimeout(hoverTimer);
      hoverTimer = null;
    }
  }

  function scheduleHoverOpen() {
    if (!canOpen) return;
    cancelHoverTimer();
    hoverTimer = setTimeout(() => {
      hoverTimer = null;
      openMenu(false);
    }, HOVER_DELAY_MS);
  }

  function close(returnFocus = false) {
    cancelHoverTimer();
    if (!open) return;
    open = false;
    if (returnFocus) triggerEl?.focus();
  }

  function selectWorld(folder: string) {
    close();
    onQuickPlayWorld(folder);
  }

  function focusItem(i: number) {
    const n = itemEls.length;
    if (n === 0) return;
    itemEls[((i % n) + n) % n]?.focus();
  }

  function onTriggerKeydown(e: KeyboardEvent) {
    if (!canOpen) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      openMenu(true);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      openMenu(false);
      queueMicrotask(() => itemEls[itemEls.length - 1]?.focus());
    } else if (e.key === 'Escape') {
      close();
    }
  }

  function onItemKeydown(e: KeyboardEvent, i: number, folder: string) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      focusItem(i + 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      focusItem(i - 1);
    } else if (e.key === 'Home') {
      e.preventDefault();
      focusItem(0);
    } else if (e.key === 'End') {
      e.preventDefault();
      focusItem(itemEls.length - 1);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      close(true);
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      selectWorld(folder);
    }
  }

  // If the instance becomes ineligible (game launches, worlds vanish) while
  // the menu is open, close it so the rendered DOM and aria-expanded stay
  // truthful — no close event is otherwise guaranteed in that case.
  $effect(() => {
    if (!canOpen) close();
  });

  // While open: close on outside pointer / scroll / resize / focus leaving.
  $effect(() => {
    if (!open) return;
    const inside = (n: Node) => !!triggerEl?.contains(n) || !!menuEl?.contains(n);
    const onPointerDown = (ev: PointerEvent) => {
      if (!inside(ev.target as Node)) close();
    };
    const onScrollOrResize = () => close();
    const onFocusIn = (ev: FocusEvent) => {
      if (!inside(ev.target as Node)) close();
    };
    window.addEventListener('pointerdown', onPointerDown, true);
    window.addEventListener('scroll', onScrollOrResize, true);
    window.addEventListener('resize', onScrollOrResize);
    document.addEventListener('focusin', onFocusIn);
    return () => {
      window.removeEventListener('pointerdown', onPointerDown, true);
      window.removeEventListener('scroll', onScrollOrResize, true);
      window.removeEventListener('resize', onScrollOrResize);
      document.removeEventListener('focusin', onFocusIn);
    };
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="relative flex-1" onmouseenter={scheduleHoverOpen} onmouseleave={() => close()}>
  <button
    bind:this={triggerEl}
    type="button"
    data-tour="play-btn"
    class="btn-success btn-lg w-full flex items-center justify-center gap-1.5 relative"
    class:rounded-b-none={open}
    aria-haspopup={canOpen ? 'menu' : undefined}
    aria-expanded={canOpen ? open : undefined}
    onclick={onPlay}
    onkeydown={onTriggerKeydown}
  >
    <Icon name="play" size={16} />
    {label}
    {#if canOpen}
      <!-- Dropdown affordance: pinned to the right edge (select-style) so it
           reads as "a worlds menu is available" without decentering the label.
           Rotates when open. -->
      <span
        class="absolute right-2.5 top-1/2 -translate-y-1/2 inline-flex transition-transform duration-150"
        class:rotate-180={open}
        aria-hidden="true"
      >
        <Icon name="chevronDown" size={16} />
      </span>
    {/if}
  </button>

  {#if open}
    <div
      bind:this={menuEl}
      role="menu"
      aria-label={menuLabel ?? label}
      tabindex="-1"
      data-testid="play-worlds-menu"
      class="fixed z-[var(--z-popover)] overflow-y-auto bg-surface border border-success border-t-0 rounded-b shadow-md py-1 outline-none"
      style="top: {top}px; left: {left}px; width: {width}px; max-height: {maxHeight}px;"
    >
      {#each worlds as w, i (w.folder_name)}
        <button
          bind:this={itemEls[i]}
          type="button"
          role="menuitem"
          tabindex="-1"
          class="w-full flex items-center gap-2 px-3 py-1.5 text-sm text-left text-secondary hover:bg-subtle focus-visible:bg-subtle focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
          onclick={() => selectWorld(w.folder_name)}
          onkeydown={(e) => onItemKeydown(e, i, w.folder_name)}
        >
          <span class="text-success inline-flex flex-shrink-0" aria-hidden="true">
            <Icon name="play" size={14} />
          </span>
          <span class="truncate">{w.folder_name}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>
