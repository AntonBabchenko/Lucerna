<script lang="ts">
  // Steam-style "results per page" control: a label followed by the page-size
  // options as inline clickable numbers (the active one highlighted), not a
  // dropdown. Shared by the mod + modpack browser pagination footers and the
  // Installed tab. `prefsKey` selects which persisted page-size it drives —
  // Browse/Modpacks use the catalog 'pageSize'; Installed uses its own
  // 'installedPageSize' (different content, different natural page size).
  import { browserPrefs, PAGE_SIZES, type PageSize } from '$lib/mods/browser-prefs.svelte';
  import { t } from '$lib/i18n';
  import SegmentedControl from '$lib/ui/SegmentedControl.svelte';

  let { prefsKey = 'pageSize' }: { prefsKey?: 'pageSize' | 'installedPageSize' } = $props();

  const options = PAGE_SIZES.map((n) => ({
    value: String(n),
    label: String(n),
    testId: `page-size-${n}`,
  }));
</script>

<span class="inline-flex items-center gap-2 text-sm">
  <span class="text-muted">{$t('mods.pageSize.perPage')}</span>
  <SegmentedControl
    {options}
    value={String(browserPrefs[prefsKey])}
    onChange={(v) => (browserPrefs[prefsKey] = Number(v) as PageSize)}
    variant="inline"
    ariaLabel={$t('mods.pageSize.perPage')}
  />
</span>
