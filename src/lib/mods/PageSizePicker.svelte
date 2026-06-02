<script lang="ts">
  // Steam-style "results per page" control: a label followed by the page-size
  // options as inline clickable numbers (the active one highlighted), not a
  // dropdown. Shared by the mod + modpack browser pagination footers.
  import { browserPrefs, PAGE_SIZES } from '$lib/mods/browser-prefs.svelte';
  import { t } from '$lib/i18n';
</script>

<span class="inline-flex items-center gap-2 text-sm">
  <span class="text-muted">{$t('mods.pageSize.perPage')}</span>
  {#each PAGE_SIZES as n}
    <button
      type="button"
      class="px-0.5 {browserPrefs.pageSize === n
        ? 'text-primary font-semibold'
        : 'text-secondary hover:text-primary'}"
      aria-pressed={browserPrefs.pageSize === n}
      data-testid="page-size-{n}"
      onclick={() => (browserPrefs.pageSize = n)}
    >
      {n}
    </button>
  {/each}
</span>
