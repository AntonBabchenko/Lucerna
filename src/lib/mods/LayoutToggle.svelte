<script lang="ts">
  import { browserPrefs } from './browser-prefs.svelte';
  import { t } from '$lib/i18n';
</script>

<div
  class="inline-flex border border-border-subtle rounded overflow-hidden"
  role="group"
  aria-label={$t('mods.layout.groupAriaLabel')}
>
  <button
    type="button"
    class="btn-secondary btn-sm rounded-none"
    class:btn-primary={browserPrefs.layout === 'grid'}
    aria-pressed={browserPrefs.layout === 'grid'}
    title={$t('mods.layout.grid')}
    data-testid="layout-grid"
    onclick={() => (browserPrefs.layout = 'grid')}>▦</button
  >
  <button
    type="button"
    class="btn-secondary btn-sm rounded-none"
    class:btn-primary={browserPrefs.layout === 'list'}
    aria-pressed={browserPrefs.layout === 'list'}
    title={$t('mods.layout.list')}
    data-testid="layout-list"
    onclick={() => (browserPrefs.layout = 'list')}>☰</button
  >
</div>
