<script lang="ts">
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import Menu from '$lib/ui/Menu.svelte';
  import type { ContextMenuItem } from '$lib/ui/menu-item';
  import { estimateMenuHeight } from '$lib/ui/menu-metrics';

  // Left-click overflow menu. A trigger button (⋯) opens a position:fixed menu
  // listing ContextMenuItems; the shared Menu owns the surface, keyboard nav, and
  // scroll/resize close. This wrapper only owns the trigger + placement (anchors
  // the menu's right edge under the trigger so it grows leftward) + focus return.
  let { items, ariaLabel }: { items: ContextMenuItem[]; ariaLabel: string } = $props();

  const WIDTH = 230;
  const MARGIN = 8;

  let open = $state(false);
  let top = $state(0);
  let left = $state(0);
  let openedByKeyboard = $state(false);
  let triggerEl: HTMLButtonElement | undefined = $state();

  function toggle(e: MouseEvent) {
    if (open) {
      close();
      return;
    }
    // Keyboard activation (Enter/Space) dispatches a click with detail === 0;
    // a real pointer click reports the click count (>= 1).
    openedByKeyboard = e.detail === 0;
    const r = triggerEl?.getBoundingClientRect();
    // Anchor the popover's right edge under the trigger so it grows leftward.
    const desiredLeft = r ? r.right - WIDTH : MARGIN;
    left = Math.min(
      Math.max(desiredLeft, MARGIN),
      Math.max(MARGIN, window.innerWidth - WIDTH - MARGIN),
    );
    const estH = estimateMenuHeight(items);
    const desiredTop = r ? r.bottom + 4 : MARGIN;
    top = Math.min(desiredTop, Math.max(MARGIN, window.innerHeight - estH - MARGIN));
    open = true;
  }

  function close() {
    open = false;
    triggerEl?.focus();
  }
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
  <Menu {items} {ariaLabel} {top} {left} width={WIDTH} onClose={close} {openedByKeyboard} />
{/if}
