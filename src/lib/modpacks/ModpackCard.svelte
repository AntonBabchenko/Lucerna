<script lang="ts">
  import type { ModpackHit } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';

  // One card in the modpack search grid (ModpackBrowseView). The card is
  // a button so the whole tile is clickable and the focus ring is
  // keyboard-reachable — clicking opens the version drawer, which is the
  // only action available on a search hit (no inline install).
  //
  // `downloads` is `number | null` on the bindings type (Modrinth omits
  // it on rare entries; CurseForge tail-section hits sometimes too), so
  // we coalesce to 0 the same way ModCard does for the mod browser.

  let {
    hit,
    onClick,
    layout = 'grid',
  }: { hit: ModpackHit; onClick: () => void; layout?: 'grid' | 'list' } = $props();
</script>

{#if layout === 'grid'}
  <button
    type="button"
    class="text-left p-3 bg-surface border rounded hover:border-accent hover:shadow-sm transition-all w-full"
    onclick={onClick}
    data-testid="modpack-card"
  >
    <div class="flex gap-3">
      {#if hit.icon_url}
        <img src={hit.icon_url} alt="" class="w-12 h-12 rounded object-cover flex-shrink-0" />
      {:else}
        <div
          class="w-12 h-12 bg-subtle rounded flex items-center justify-center text-placeholder flex-shrink-0"
        >
          <Icon name="package" size={24} />
        </div>
      {/if}
      <div class="min-w-0 flex-1">
        <div
          class="font-semibold text-sm truncate"
          use:tooltip={{ text: hit.title, whenOverflowing: true }}
        >
          {hit.title}
        </div>
        <div class="text-xs text-muted line-clamp-2">{hit.description}</div>
        <div class="text-xs text-placeholder mt-1">
          {$t('modpacks.card.downloads', { count: (hit.downloads ?? 0).toLocaleString() })}
        </div>
        {#if hit.distribution_allowed === false}
          <div
            class="mt-1 inline-block text-xs px-1.5 py-0.5 rounded bg-warning-bg text-warning-text"
          >
            {$t('modpacks.card.distributionDisabled')}
          </div>
        {/if}
      </div>
    </div>
  </button>
{:else}
  <button
    type="button"
    class="flex items-center gap-3 px-3 py-2 border-b border-border-subtle bg-surface hover:bg-subtle transition-colors text-left w-full"
    onclick={onClick}
    data-testid="card-list-row"
  >
    {#if hit.icon_url}
      <img src={hit.icon_url} alt="" class="w-8 h-8 rounded object-cover flex-shrink-0" />
    {:else}
      <div
        class="w-8 h-8 bg-subtle rounded flex items-center justify-center text-placeholder text-xs flex-shrink-0"
      >
        <Icon name="package" size={16} />
      </div>
    {/if}
    <span
      class="font-medium text-sm truncate flex-1"
      use:tooltip={{ text: hit.title, whenOverflowing: true }}>{hit.title}</span
    >
    <span class="text-xs text-placeholder flex-shrink-0"
      >{$t('modpacks.card.downloadsShort', { count: (hit.downloads ?? 0).toLocaleString() })}</span
    >
    {#if hit.distribution_allowed === false}
      <span class="text-xs px-1.5 py-0.5 rounded bg-warning-bg text-warning-text flex-shrink-0">
        {$t('modpacks.card.distributionDisabledShort')}
      </span>
    {/if}
  </button>
{/if}
