<script lang="ts">
  // Test fixture: two ContextualTour instances mounted in ONE flush.
  //
  // Two separate `render()` calls each flush on their own, so the first tour's
  // body-attribute effect has already run by the time the second mounts — that
  // arrangement cannot reproduce same-flush co-activation. Mounting both as
  // children of one component does: every child's onMount runs in the same
  // flush pass, before any effect re-runs, so a guard that only reads the DOM
  // attribute is blind to the sibling that just claimed the screen.
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import { LOGS_STEPS, MANAGE_STEPS } from '$lib/onboarding/contextual-tours';
</script>

<ContextualTour id="manage" steps={MANAGE_STEPS} />
<ContextualTour id="logs" steps={LOGS_STEPS} />
