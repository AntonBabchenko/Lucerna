<script lang="ts">
  // Settings → General. Currently hosts only the "Replay onboarding
  // tour" affordance; future general settings (theme toggle,
  // language, update prefs) accumulate here per post-v0.5.0 backlog
  // item 6.
  import { replayTour } from '$lib/onboarding/state.svelte';
  import { settingsOpen } from './state.svelte';

  function onReplay() {
    replayTour();
    settingsOpen.value = null;
  }
</script>

<section class="flex flex-col gap-3">
  <h3 class="font-medium text-sm">Onboarding</h3>
  <div class="flex items-center gap-3">
    <button
      type="button"
      class="border rounded px-3 py-1.5 text-sm hover:bg-neutral-50"
      onclick={onReplay}
    >
      Replay onboarding tour
    </button>
    <p class="text-xs text-neutral-500">Show the 6-step tutorial again.</p>
  </div>
</section>
