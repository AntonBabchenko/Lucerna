<script lang="ts">
  // Steam-style "results per page" control: a label followed by the page-size
  // options as inline clickable numbers (the active one highlighted), not a
  // dropdown. Shared by the mod + modpack browser pagination footers and the
  // Installed tab. `prefsKey` selects which persisted page-size it drives —
  // Browse/Modpacks use the catalog 'pageSize'; Installed uses its own
  // 'installedPageSize' (different content, different natural page size).
  import { browserPrefs, PAGE_SIZES } from '$lib/mods/browser-prefs.svelte';
  import { t } from '$lib/i18n';

  let { prefsKey = 'pageSize' }: { prefsKey?: 'pageSize' | 'installedPageSize' } = $props();
</script>

<span class="inline-flex items-center gap-2 text-sm">
  <span class="text-muted">{$t('mods.pageSize.perPage')}</span>
  {#each PAGE_SIZES as n (n)}
    <button
      type="button"
      class="px-0.5 {browserPrefs[prefsKey] === n
        ? 'text-primary font-semibold'
        : 'text-secondary hover:text-primary'}"
      aria-pressed={browserPrefs[prefsKey] === n}
      data-testid="page-size-{n}"
      onclick={() => (browserPrefs[prefsKey] = n)}
    >
      {n}
    </button>
  {/each}
</span>
