<script lang="ts">
  // The single fixed-position tooltip bubble. Mounted once in +layout.svelte.
  // Reads the shared controller state, measures its own rendered size, and asks
  // the controller to finalize the position (flip/clamp + caret offset). One
  // bubble in the DOM, at most one visible at a time.
  //
  // Style: surface-coloured bubble with a caret pointing at the trigger, a soft
  // shadow, and a subtle fade+scale entrance that grows from the trigger side.
  import { positionTooltip, TOOLTIP_ID, tooltipState } from './tooltip-controller.svelte';

  let bubble = $state<HTMLDivElement | undefined>();

  // Respect reduced motion. The app's global reduced-motion CSS only reaches
  // CSS transitions/animations; Svelte transitions run via the Web Animations
  // API, so they need this explicit gate.
  const reduced =
    typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

  // After the bubble renders with its text, measure it and finalize position.
  $effect(() => {
    if (tooltipState.visible && bubble) {
      positionTooltip({ width: bubble.offsetWidth, height: bubble.offsetHeight });
    }
  });

  // Entrance: fade + a hair of scale + a few px rise from the trigger side.
  // `dir` makes the bubble grow up from a trigger below it and down from one above.
  function reveal(_node: Element, { placement }: { placement: 'top' | 'bottom' }) {
    const dir = placement === 'top' ? 1 : -1;
    return {
      duration: reduced ? 0 : 120,
      css: (t: number, u: number) =>
        `opacity:${t}; transform: translateY(${u * 3 * dir}px) scale(${0.97 + 0.03 * t});`,
    };
  }
</script>

{#if tooltipState.visible}
  <div
    bind:this={bubble}
    id={TOOLTIP_ID}
    role="tooltip"
    transition:reveal={{ placement: tooltipState.placement }}
    class="fixed z-[var(--z-tooltip)] max-w-xs pointer-events-none normal-case tracking-normal
           rounded-md border border-border-subtle bg-surface px-2.5 py-1.5
           text-xs font-medium leading-snug text-secondary shadow-lg"
    style="top: {tooltipState.top}px; left: {tooltipState.left}px; transform-origin: {tooltipState.placement ===
    'top'
      ? 'bottom'
      : 'top'} {tooltipState.caretLeft}px;"
  >
    {tooltipState.text}
    <span
      aria-hidden="true"
      class="absolute h-2 w-2 rotate-45 bg-surface border-border-subtle {tooltipState.placement ===
      'top'
        ? '-bottom-1 border-b border-r'
        : '-top-1 border-t border-l'}"
      style="left: {tooltipState.caretLeft}px; margin-left: -4px;"
    ></span>
  </div>
{/if}
