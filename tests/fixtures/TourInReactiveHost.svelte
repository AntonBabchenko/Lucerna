<script lang="ts">
  // Test fixture: a ContextualTour whose HOST is torn down by the very state
  // change that activates the main tour — the production replay path, where
  // `replayTour()` calls `serversUi.setMode('client')` and sets
  // `tourState.active = true` in one synchronous block, unmounting every
  // servers-mode host in that same flush.
  //
  // An imperative `unmount()` cannot reproduce it: testing-library's unmount is
  // `flushSync(() => unmount(component))`, so the tour's pending yield effect
  // DRAINS FIRST and `active` is already false by the time onDestroy looks —
  // which makes `if (active && !tourState.active)` and a bare `if (active)`
  // indistinguishable. Here the destroy is a block (render) effect, so it runs
  // before any user `$effect` of this flush: the pending yield is discarded and
  // onDestroy sees `active === true` with the main tour up, which is exactly
  // the case the guard exists for.
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import { MANAGE_STEPS } from '$lib/onboarding/contextual-tours';
  import { tourState } from '$lib/onboarding/state.svelte';
</script>

{#if !tourState.active}
  <ContextualTour id="manage" steps={MANAGE_STEPS} />
{/if}
