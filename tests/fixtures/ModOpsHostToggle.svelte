<script lang="ts">
  // Test fixture: ModOpsHost inside an {#if} the test flips, so the host is torn down by a block
  // effect in the flush that follows the change — the way an ancestor removes it in the app. An
  // imperative unmount() cannot stand in for that: testing-library wraps it in flushSync, which
  // drains pending effects before the destroy (see TourInReactiveHost.svelte).
  import ModOpsHost from '$lib/mods/ops/ModOpsHost.svelte';

  let { shown }: { shown: boolean } = $props();
</script>

{#if shown}
  <ModOpsHost activeInstanceId="inst" />
{/if}
