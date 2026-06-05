<script lang="ts">
  // Generic always-visible "(?)" help popover used next to section / modal
  // headers. The popover is `position: fixed` (anchored to the viewport), not
  // `absolute`: a host like the sidebar is `overflow-y-auto`, which per the CSS
  // overflow spec also forces `overflow-x: auto`, so an absolute popover wider
  // than the host would be clipped and add a horizontal scrollbar. A fixed
  // popover escapes the host's overflow box; its position is measured from the
  // trigger on open and clamped into the viewport.
  //
  // The trigger is elevated (z-50) ONLY WHILE OPEN. A persistent z-index made
  // the small "(?)" glyph paint through modals layered above its host (e.g. the
  // sidebar tooltip showing over an open dialog). While closed it stays in
  // normal flow, so any modal correctly covers it.
  import CloseButton from '$lib/ui/CloseButton.svelte';

  let {
    body,
    triggerAriaLabel,
    triggerTitle = undefined,
    closeAriaLabel,
    width = 260,
  }: {
    body: string;
    triggerAriaLabel: string;
    triggerTitle?: string | undefined;
    closeAriaLabel: string;
    width?: number;
  } = $props();

  const GAP = 4;
  const MARGIN = 8;
  const popoverId = `help-popover-${crypto.randomUUID()}`;

  let open = $state(false);
  let trigger: HTMLButtonElement | undefined;
  let popoverTop = $state(0);
  let popoverLeft = $state(0);

  function positionPopover() {
    if (!trigger) return;
    const r = trigger.getBoundingClientRect();
    popoverTop = r.bottom + GAP;
    const maxLeft = window.innerWidth - width - MARGIN;
    popoverLeft = Math.min(Math.max(r.left, MARGIN), Math.max(MARGIN, maxLeft));
  }

  function toggle() {
    if (open) {
      open = false;
    } else {
      positionPopover();
      open = true;
    }
  }

  // A fixed popover does not follow the trigger when the layout shifts, so close
  // it on scroll/resize while open. `scroll` is captured (third arg `true`) so
  // it also catches a scrollable host's own scroll (scroll does not bubble).
  $effect(() => {
    if (!open) return;
    const close = () => (open = false);
    window.addEventListener('scroll', close, true);
    window.addEventListener('resize', close);
    return () => {
      window.removeEventListener('scroll', close, true);
      window.removeEventListener('resize', close);
    };
  });
</script>

<div class="relative inline-block">
  <button
    bind:this={trigger}
    type="button"
    class="relative text-xs text-placeholder hover:text-secondary leading-none px-1"
    class:z-50={open}
    aria-label={triggerAriaLabel}
    title={triggerTitle}
    aria-expanded={open}
    aria-controls={popoverId}
    onclick={toggle}
  >
    (?)
  </button>
  {#if open}
    <!-- Click-outside backdrop -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div role="presentation" class="fixed inset-0 z-30" onclick={() => (open = false)}></div>
    <div
      id={popoverId}
      class="fixed z-40 normal-case tracking-normal bg-surface border border-border-subtle rounded shadow-md p-2.5"
      style="top: {popoverTop}px; left: {popoverLeft}px; width: {width}px;"
    >
      <div class="absolute top-1 right-1">
        <CloseButton onClick={() => (open = false)} ariaLabel={closeAriaLabel} />
      </div>
      <p class="text-xs text-secondary leading-snug pr-6">{body}</p>
    </div>
  {/if}
</div>
