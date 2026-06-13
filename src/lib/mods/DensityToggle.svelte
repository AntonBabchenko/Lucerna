<script lang="ts">
  import { browserPrefs } from './browser-prefs.svelte';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
</script>

<div
  class="inline-flex border border-border-subtle rounded overflow-hidden"
  role="group"
  aria-label={$t('mods.density.groupAriaLabel')}
>
  <button
    type="button"
    class="btn-secondary btn-sm rounded-none"
    class:btn-primary={browserPrefs.density === 'comfortable'}
    aria-pressed={browserPrefs.density === 'comfortable'}
    aria-label={$t('mods.density.comfortable')}
    title={$t('mods.density.comfortable')}
    data-testid="density-comfortable"
    onclick={() => (browserPrefs.density = 'comfortable')}><Icon name="densityComfortable" /></button
  >
  <button
    type="button"
    class="btn-secondary btn-sm rounded-none"
    class:btn-primary={browserPrefs.density === 'compact'}
    aria-pressed={browserPrefs.density === 'compact'}
    aria-label={$t('mods.density.compact')}
    title={$t('mods.density.compact')}
    data-testid="density-compact"
    onclick={() => (browserPrefs.density = 'compact')}><Icon name="densityCompact" /></button
  >
</div>
