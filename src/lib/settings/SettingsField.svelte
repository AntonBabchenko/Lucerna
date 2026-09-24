<script lang="ts">
  // Wraps a searchable settings control (label + control together, so the flash
  // ring encloses both — the contract field-flash.ts documents). Stamps a stable
  // data-search-anchor for tests/selectors and flashes when the shared
  // settingsSearchFocus rune points at this anchor. The rune is consumed once
  // the flash has been delivered: a re-mount of this field (returning to the
  // tab) must not flash again, and the next open must start clean. A jump
  // from inside the modal (settingsJumpFocus) also takes keyboard focus.
  import type { Snippet } from 'svelte';
  import { fieldFlash } from '$lib/ui/field-flash';
  import { shouldFocusAnchor, type SettingsAnchor } from './search-index';
  import { settingsJumpFocus, settingsSearchFocus } from './state.svelte';

  let { anchor, children }: { anchor: SettingsAnchor; children: Snippet } = $props();

  function consume() {
    if (settingsSearchFocus.value === anchor) settingsSearchFocus.value = null;
    if (settingsJumpFocus.value === anchor) settingsJumpFocus.value = null;
  }
</script>

<!-- scroll-mt-4: a block taller than the column aligns to its start, and the
     ring's outline-offset would sit on the scrollport edge without it. -->
<div
  class="scroll-mt-4"
  data-search-anchor={anchor}
  use:fieldFlash={{
    active: settingsSearchFocus.value === anchor,
    focus: shouldFocusAnchor(anchor) || settingsJumpFocus.value === anchor,
    onDelivered: consume,
  }}
>
  {@render children()}
</div>
