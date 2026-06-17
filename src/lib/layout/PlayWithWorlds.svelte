<script lang="ts">
  import { Icon } from '$lib/ui/icons';
  import type { WorldQuickEntry } from '$lib/ipc/bindings';

  // The green Play button plus a hover/keyboard popover of the instance's
  // worlds. Single-click launches normally (onPlay); the popover lets the
  // user jump straight into a world. The popover only exists when
  // `menuEnabled` (Quick Play supported + instance ready + not running,
  // decided by the page) AND there is at least one world. position:fixed +
  // bottom-anchor so it escapes the sidebar's overflow box and opens upward
  // (the Play button sits low). Mirrors OverflowMenu's close-on-scroll/resize.
  let {
    worlds,
    onPlay,
    onQuickPlayWorld,
    menuEnabled,
    label,
  }: {
    worlds: WorldQuickEntry[];
    onPlay: () => void;
    onQuickPlayWorld: (folderName: string) => void;
    menuEnabled: boolean;
    label: string;
  } = $props();

  const HOVER_DELAY_MS = 200;
  const MARGIN = 8;
  const GAP = 4;
  const MIN_WIDTH = 220;

  let open = $state(false);
  let bottom = $state(0);
  let left = $state(0);
  let width = $state(MIN_WIDTH);
  let hoverTimer: ReturnType<typeof setTimeout> | null = null;
  let triggerEl = $state<HTMLButtonElement>();
  let menuEl = $state<HTMLDivElement>();
  let itemEls = $state<HTMLButtonElement[]>([]);

  const canOpen = $derived(menuEnabled && worlds.length > 0);

  function place() {
    const r = triggerEl?.getBoundingClientRect();
    if (!r) return;
    width = Math.max(r.width, MIN_WIDTH);
    left = Math.min(Math.max(r.left, MARGIN), Math.max(MARGIN, window.innerWidth - width - MARGIN));
    // Anchor the menu's bottom edge just above the trigger → opens upward.
    bottom = Math.max(MARGIN, window.innerHeight - r.top + GAP);
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
    class="btn-success btn-lg w-full flex items-center justify-center gap-1.5"
    aria-haspopup={canOpen ? 'menu' : undefined}
    aria-expanded={canOpen ? open : undefined}
    onclick={onPlay}
    onkeydown={onTriggerKeydown}
  >
    <Icon name="play" size={16} />
    {label}
  </button>

  {#if open}
    <div
      bind:this={menuEl}
      role="menu"
      aria-label={label}
      tabindex="-1"
      data-testid="play-worlds-menu"
      class="fixed z-50 overflow-y-auto bg-surface border border-border-emphasis rounded shadow-md py-1 outline-none"
      style="bottom: {bottom}px; left: {left}px; width: {width}px; max-height: 50vh;"
    >
      {#each worlds as w, i (w.folder_name)}
        <button
          bind:this={itemEls[i]}
          type="button"
          role="menuitem"
          tabindex="-1"
          class="w-full flex items-center gap-2 px-3 py-1.5 text-sm text-left text-secondary hover:bg-subtle focus:bg-subtle outline-none"
          onclick={() => selectWorld(w.folder_name)}
          onkeydown={(e) => onItemKeydown(e, i, w.folder_name)}
        >
          <Icon name="play" size={14} />
          <span class="truncate">{w.folder_name}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>
