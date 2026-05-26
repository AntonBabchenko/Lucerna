<script lang="ts">
  // Always-visible (?) explainer next to the Sidebar "Instance"
  // section header. Safety net for users who skipped the tour or
  // upgraded from v0.4.x with existing instances.
  //
  // Body copy is a tightened standalone variant of the tour's
  // step-1 welcome text — deliberately NOT shared with steps.ts,
  // since the popover wants a single concise sentence while the
  // welcome step wants a brand-led intro.
  //
  // The popover is `position: fixed` (anchored to the viewport), not
  // `absolute`: the host sidebar is `overflow-y-auto`, and per the CSS
  // overflow spec that forces `overflow-x` to `auto` too — an absolute
  // popover wider than the sidebar would be clipped by it and would
  // add a horizontal scrollbar. A fixed popover escapes the sidebar's
  // overflow box. Its position is measured from the trigger each time
  // it opens and clamped into the viewport.

  // Keep POPOVER_WIDTH in sync with the `w-[260px]` class on the popover.
  const POPOVER_WIDTH = 260;
  const GAP = 4;
  const MARGIN = 8;

  let open = $state(false);
  let trigger: HTMLButtonElement | undefined;
  let popoverTop = $state(0);
  let popoverLeft = $state(0);

  function positionPopover() {
    if (!trigger) return;
    const r = trigger.getBoundingClientRect();
    popoverTop = r.bottom + GAP;
    const maxLeft = window.innerWidth - POPOVER_WIDTH - MARGIN;
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

  // A fixed popover does not follow the trigger when the layout
  // shifts, so close it on scroll/resize while it is open. `scroll`
  // is captured (third arg `true`) so it also catches the sidebar's
  // own scroll — scroll events do not bubble.
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
    class="relative z-50 text-xs text-placeholder hover:text-secondary leading-none px-1"
    aria-label="What is an instance?"
    title="What is an instance?"
    aria-expanded={open}
    aria-controls="instance-concept-popover"
    onclick={toggle}
  >
    (?)
  </button>
  {#if open}
    <!-- Click-outside backdrop -->
    <button
      type="button"
      class="fixed inset-0 z-30"
      aria-label="Close instance concept tooltip"
      onclick={() => (open = false)}
    ></button>
    <div
      id="instance-concept-popover"
      class="fixed z-40 w-[260px] bg-surface border border-border-subtle rounded shadow-md p-2.5"
      style="top: {popoverTop}px; left: {popoverLeft}px;"
    >
      <p class="text-xs text-secondary leading-snug">
        Each instance is a self-contained world — its own Minecraft version, loader, mods, configs
        and saves. Switching instance = switching Minecraft install, without touching the others.
      </p>
    </div>
  {/if}
</div>
