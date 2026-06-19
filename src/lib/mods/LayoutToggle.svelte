<script lang="ts">
  import { browserPrefs } from './browser-prefs.svelte';
  import { t } from '$lib/i18n';
  import SegmentedControl from '$lib/ui/SegmentedControl.svelte';

  const options = $derived([
    { value: 'grid', label: $t('mods.layout.grid'), icon: 'grid' as const, testId: 'layout-grid' },
    { value: 'list', label: $t('mods.layout.list'), icon: 'list' as const, testId: 'layout-list' },
  ]);
</script>

<SegmentedControl
  {options}
  value={browserPrefs.layout}
  onChange={(v) => (browserPrefs.layout = v as 'grid' | 'list')}
  variant="boxed"
  ariaLabel={$t('mods.layout.groupAriaLabel')}
/>
