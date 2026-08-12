<script lang="ts">
  // Wraps a searchable settings control (label + control together, so the flash
  // ring encloses both — the contract field-flash.ts documents). Stamps a stable
  // data-search-anchor for tests/selectors and flashes when the shared
  // settingsSearchFocus rune points at this anchor.
  import type { Snippet } from 'svelte';
  import { fieldFlash } from '$lib/ui/field-flash';
  import { shouldFocusAnchor, type SettingsAnchor } from './search-index';
  import { settingsSearchFocus } from './state.svelte';

  let { anchor, children }: { anchor: SettingsAnchor; children: Snippet } = $props();
</script>

<div
  data-search-anchor={anchor}
  use:fieldFlash={{
    active: settingsSearchFocus.value === anchor,
    focus: shouldFocusAnchor(anchor),
  }}
>
  {@render children()}
</div>
