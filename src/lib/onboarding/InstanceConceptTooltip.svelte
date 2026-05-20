<script lang="ts">
  // Always-visible (?) explainer next to the Sidebar "Instance"
  // section header. Safety net for users who skipped the tour or
  // upgraded from v0.4.x with existing instances.
  //
  // Body copy is a tightened standalone variant of the tour's
  // step-1 welcome text — deliberately NOT shared with steps.ts,
  // since the popover wants a single concise sentence while the
  // welcome step wants a brand-led intro.
  let open = $state(false);
</script>

<div class="relative inline-block">
  <button
    type="button"
    class="relative z-50 text-xs text-neutral-400 hover:text-neutral-700 leading-none px-1"
    aria-label="What is an instance?"
    title="What is an instance?"
    aria-expanded={open}
    aria-controls="instance-concept-popover"
    onclick={() => (open = !open)}
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
      class="absolute z-40 top-5 left-0 w-[260px] bg-white border border-neutral-200 rounded shadow-md p-2.5"
    >
      <p class="text-xs text-neutral-700 leading-snug">
        Each instance is a self-contained world — its own Minecraft version, loader, mods, configs
        and saves. Switching instance = switching Minecraft install, without touching the others.
      </p>
    </div>
  {/if}
</div>
