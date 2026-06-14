<script lang="ts">
  import type { ModpackHit } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import CardMedia from '$lib/ui/cards/CardMedia.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';

  // One card in the modpack search grid/list (ModpackBrowseView). The whole card
  // is a button — clicking opens the version drawer, the only action on a search
  // hit. Composes the shared CardMedia + StatusBadge primitives.
  let {
    hit,
    onClick,
    layout = 'grid',
  }: { hit: ModpackHit; onClick: () => void; layout?: 'grid' | 'list' } = $props();
</script>

{#if layout === 'grid'}
  <button
    type="button"
    class="text-left p-3 bg-surface border border-border-subtle rounded-lg hover:border-accent hover:bg-subtle transition-colors w-full"
    onclick={onClick}
    data-testid="modpack-card"
  >
    <div class="flex gap-3">
      <CardMedia iconUrl={hit.icon_url} placeholder="package" size="lg" />
      <div class="min-w-0 flex-1">
        <div class="font-semibold text-sm truncate">{hit.title}</div>
        <div class="text-xs text-muted line-clamp-2">{hit.description}</div>
        <div class="flex items-center gap-1.5 mt-1.5 flex-wrap">
          <StatusBadge variant="neutral" icon="download">
            {$t('modpacks.card.downloads', { count: (hit.downloads ?? 0).toLocaleString() })}
          </StatusBadge>
          {#if hit.distribution_allowed === false}
            <StatusBadge variant="warning" icon="warning"
              >{$t('modpacks.card.distributionDisabled')}</StatusBadge
            >
          {/if}
        </div>
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
    <CardMedia iconUrl={hit.icon_url} placeholder="package" size="md" />
    <span class="font-medium text-sm truncate flex-1">{hit.title}</span>
    <span class="text-xs text-placeholder flex-shrink-0 inline-flex items-center gap-1">
      <Icon name="download" size={12} />
      {$t('modpacks.card.downloadsShort', { count: (hit.downloads ?? 0).toLocaleString() })}
    </span>
    {#if hit.distribution_allowed === false}
      <StatusBadge variant="warning">{$t('modpacks.card.distributionDisabledShort')}</StatusBadge>
    {/if}
  </button>
{/if}
