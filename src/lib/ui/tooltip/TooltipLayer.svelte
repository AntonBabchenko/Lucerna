<script lang="ts">
  // The single fixed-position tooltip bubble. Mounted once in +layout.svelte.
  // Reads the shared controller state, measures its own rendered size, and asks
  // the controller to finalize the position (flip/clamp). One bubble in the DOM,
  // at most one visible at a time.
  import { fade } from 'svelte/transition';
  import { positionTooltip, TOOLTIP_ID, tooltipState } from './tooltip-controller.svelte';

  let bubble = $state<HTMLDivElement | undefined>();

  // Respect reduced motion: no fade when the user asked for less motion.
  const reduced =
    typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

  // After the bubble renders with its text, measure it and finalize position.
  $effect(() => {
    if (tooltipState.visible && bubble) {
      positionTooltip({ width: bubble.offsetWidth, height: bubble.offsetHeight });
    }
  });
</script>

{#if tooltipState.visible}
  <div
    bind:this={bubble}
    id={TOOLTIP_ID}
    role="tooltip"
    transition:fade={{ duration: reduced ? 0 : 100 }}
    class="fixed z-[210] max-w-xs pointer-events-none normal-case tracking-normal
           rounded border border-border-subtle bg-surface px-2 py-1
           text-xs leading-snug text-secondary shadow-md"
    style="top: {tooltipState.top}px; left: {tooltipState.left}px;"
  >
    {tooltipState.text}
  </div>
{/if}
