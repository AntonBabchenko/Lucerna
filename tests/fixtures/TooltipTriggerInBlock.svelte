<script lang="ts">
  // Test fixture: a focused tooltip trigger that a block takes away — the × of the last toast
  // (ToastHost's {#each}), any button under an {#if}. The block removes it inside its own effect,
  // and Chromium fires `focusout` on the focused button right there, synchronously (the test
  // restores that order; happy-dom only forgets the focus). testing-library's `unmount()` flushes
  // on its own, outside any block, so it cannot put the removal where the error came from.
  import { tooltip } from '$lib/ui/tooltip';

  let shown = $state(true);
</script>

{#if shown}
  <button type="button" aria-label="Close" use:tooltip={'Close'} onclick={() => (shown = false)}
    >×</button
  >
{/if}
