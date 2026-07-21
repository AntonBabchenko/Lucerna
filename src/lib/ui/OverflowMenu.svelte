<script lang="ts">
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import Menu from '$lib/ui/Menu.svelte';
  import type { ContextMenuItem } from '$lib/ui/cards/ContextMenu.svelte';

  // Left-click overflow menu. A trigger button (⋯) opens a position:fixed menu
  // listing ContextMenuItems; the shared Menu owns the surface, keyboard nav, and
  // scroll/resize close. This wrapper only owns the trigger + placement (anchors
  // the menu's right edge under the trigger so it grows leftward) + focus return.
  let { items, ariaLabel }: { items: ContextMenuItem[]; ariaLabel: string } = $props();

  const WIDTH = 230;
  const MARGIN = 8;
  const ROW = 34;

  let open = $state(false);
  let top = $state(0);
  let left = $state(0);
  let triggerEl: HTMLButtonElement | undefined = $state();

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
  <Menu {items} {ariaLabel} {top} {left} width={WIDTH} onClose={close} />
{/if}
