<script lang="ts">
  // Test fixture: a ContextualTour over two anchors, the second rendered only while `open` — the
  // way the deps tour's Requires block exists only once its host expanded the panel from `onStep`.
  // `onStep` READS `open` before writing it, as the host's expand does, so a missing `untrack`
  // would subscribe the tour's effect to it.
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import type { TourStep } from '$lib/onboarding/steps';

  let { steps, spy }: { steps: ReadonlyArray<TourStep>; spy: (index: number) => void } = $props();
  let open = $state(false);

  function onStep(index: number) {
    spy(index);
    if (index >= 1 && !open) open = true;
  }
</script>

<div data-tour-ctx="hook-a">Anchor A</div>
{#if open}<div data-tour-ctx="hook-b">Anchor B</div>{/if}
<button type="button" onclick={() => (open = !open)}>Toggle B</button>
<ContextualTour id="manage" {steps} {onStep} />
