<script lang="ts">
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import type { ContextMenuItem } from '$lib/ui/cards/ContextMenu.svelte';

  // Left-click overflow menu. A trigger button (⋯) opens a position:fixed
  // popover listing ContextMenuItems. The fixed positioning escapes any host
  // overflow box (mirrors ContextMenu / HelpPopover). Keyboard nav, scrim
  // close, scroll/resize close, and focus return are all handled here.
  let {
    items,
    ariaLabel,
  }: { items: ContextMenuItem[]; ariaLabel: string } = $props();

  const WIDTH = 230;
  const MARGIN = 8;
  const ROW = 34;

  let open = $state(false);
  let top = $state(0);
  let left = $state(0);
  let activeIndex = $state(-1);
  let menuEl: HTMLDivElement | undefined = $state();
  let triggerEl: HTMLButtonElement | undefined = $state();

  const enabledIndexes = $derived(
    items.map((it, i) => (it.disabled ? -1 : i)).filter((i) => i >= 0),
  );

  function toggle() {
    if (open) {
      close();
      return;
    }
    const r = triggerEl?.getBoundingClientRect();
    // Anchor the popover's right edge under the trigger so it grows leftward.
    const desiredLeft = r ? r.right - WIDTH : MARGIN;
    left = Math.min(
      Math.max(desiredLeft, MARGIN),
      Math.max(MARGIN, window.innerWidth - WIDTH - MARGIN),
    );
    const estH = items.length * ROW + 10;
    const desiredTop = r ? r.bottom + 4 : MARGIN;
    top = Math.min(desiredTop, Math.max(MARGIN, window.innerHeight - estH - MARGIN));
    open = true;
    activeIndex = enabledIndexes[0] ?? -1;
  }

  function close() {
    open = false;
    activeIndex = -1;
    triggerEl?.focus();
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

<button
  bind:this={triggerEl}
  type="button"
  class="btn-icon"
  aria-haspopup="menu"
  aria-expanded={open}
  aria-label={ariaLabel}
  use:tooltip={ariaLabel}
  onclick={toggle}
>
  <Icon name="moreVertical" />
</button>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div role="presentation" class="fixed inset-0 z-40" onclick={close}></div>
  <div
    bind:this={menuEl}
    role="menu"
    tabindex="-1"
    aria-label={ariaLabel}
    class="fixed z-50 max-h-[80vh] overflow-y-auto bg-surface border border-border-emphasis rounded shadow-md py-1 outline-none"
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
{/if}
