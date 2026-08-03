<script lang="ts">
  import type { ModpackHit } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import CardMedia from '$lib/ui/cards/CardMedia.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';

  // One card in the modpack search grid/list (ModpackBrowseView). The card body
  // is a button — clicking opens the version drawer. When `onQuickInstall` is
  // provided, a non-nested action button is added (overlay in grid, trailing in
  // list) that installs the latest version directly, mirroring the mods browser.
  // Composes the shared CardMedia + StatusBadge primitives.
  let {
    hit,
    onClick,
    layout = 'grid',
    onQuickInstall = null,
    installing = false,
    quickInstallDisabledReason = null,
  }: {
    hit: ModpackHit;
    onClick: () => void;
    layout?: 'grid' | 'list';
    onQuickInstall?: (() => void) | null;
    installing?: boolean;
    // §7 fallback gating: non-null while the data root is unavailable, which
    // blocks the quick-install action (it creates a new instance) and
    // explains why via the tooltip. See data-root-gating.ts.
    quickInstallDisabledReason?: string | null;
  } = $props();
</script>

{#snippet quickInstallButton()}
  <button
    type="button"
    class="btn-icon btn-icon-sm !text-accent"
    disabled={installing || quickInstallDisabledReason !== null}
    aria-label={$t('modpacks.card.quickInstall')}
    use:tooltip={{
      text: quickInstallDisabledReason ?? $t('modpacks.card.quickInstall'),
      describe: false,
    }}
    onclick={(e) => {
      e.stopPropagation();
      onQuickInstall?.();
    }}
  >
    {#if installing}<Spinner size="sm" />{:else}<Icon name="download" size={15} />{/if}
  </button>
{/snippet}

{#if layout === 'grid'}
  <div class="relative">
    <button
      type="button"
      class="text-left p-3 bg-surface border border-border-subtle rounded-lg hover:border-accent hover:bg-subtle transition-colors w-full"
      onclick={onClick}
      data-testid="modpack-card"
    >
      <div class="flex gap-3">
        <CardMedia iconUrl={hit.icon_url} placeholder="package" size="lg" />
        <div class="min-w-0 flex-1" class:pr-7={onQuickInstall != null}>
          <div class="font-semibold text-sm truncate">{hit.title}</div>
          {#if hit.author}
            <div class="text-xs text-muted flex items-center gap-1 min-w-0">
              <Icon name="user" size={12} class="flex-shrink-0" />
              <span class="truncate">{$t('modpacks.card.byAuthor', { author: hit.author })}</span>
            </div>
          {/if}
          <div class="text-xs text-muted line-clamp-2">{hit.description}</div>
          <div class="flex items-center gap-1.5 mt-1.5 flex-wrap">
            <!-- Raw count, never pre-formatted: the message is an ICU plural, so
                 the formatter needs a number to pick the category, and it groups
                 the digits itself using the UI locale (toLocaleString() would use
                 the OS one). -->
            <StatusBadge variant="neutral" icon="download">
              {$t('modpacks.card.downloads', { count: hit.downloads ?? 0 })}
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
    {#if onQuickInstall}
      <div class="absolute top-2 right-2">{@render quickInstallButton()}</div>
    {/if}
  </div>
{:else}
  <div
    class="flex items-center gap-3 px-3 py-2 border-b border-border-subtle bg-surface hover:bg-subtle transition-colors"
  >
    <button
      type="button"
      class="flex items-center gap-3 flex-1 min-w-0 text-left"
      onclick={onClick}
      data-testid="card-list-row"
    >
      <CardMedia iconUrl={hit.icon_url} placeholder="package" size="md" />
      <span class="font-medium text-sm truncate flex-1">{hit.title}</span>
      {#if hit.author}
        <span
          class="text-xs text-placeholder flex-shrink-0 inline-flex items-center gap-1 min-w-0 max-w-[10rem]"
        >
          <Icon name="user" size={12} class="flex-shrink-0" />
          <span class="truncate">{hit.author}</span>
        </span>
      {/if}
      <span class="text-xs text-placeholder flex-shrink-0 inline-flex items-center gap-1">
        <Icon name="download" size={12} />
        {$t('modpacks.card.downloadsShort', { count: hit.downloads ?? 0 })}
      </span>
      {#if hit.distribution_allowed === false}
        <StatusBadge variant="warning">{$t('modpacks.card.distributionDisabledShort')}</StatusBadge>
      {/if}
    </button>
    {#if onQuickInstall}
      {@render quickInstallButton()}
    {/if}
  </div>
{/if}
