<script lang="ts">
  import { browserPrefs } from './browser-prefs.svelte';
</script>

<div
  class="inline-flex border border-border-subtle rounded overflow-hidden"
  role="group"
  aria-label="Result layout"
>
  <button
    type="button"
    class="btn-secondary btn-sm rounded-none"
    class:btn-primary={browserPrefs.layout === 'grid'}
    aria-pressed={browserPrefs.layout === 'grid'}
    title="Grid"
    data-testid="layout-grid"
    onclick={() => (browserPrefs.layout = 'grid')}>▦</button
  >
  <button
    type="button"
    class="btn-secondary btn-sm rounded-none"
    class:btn-primary={browserPrefs.layout === 'list'}
    aria-pressed={browserPrefs.layout === 'list'}
    title="List"
    data-testid="layout-list"
    onclick={() => (browserPrefs.layout = 'list')}>☰</button
  >
</div>
