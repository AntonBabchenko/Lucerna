<script lang="ts">
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { Icon, type IconName } from '$lib/ui/icons';
  import type { AttentionItem, AttentionKind } from './attention';

  let { items, onAction }: { items: AttentionItem[]; onAction: (kind: AttentionKind) => void } =
    $props();

  const TEXT_KEY: Record<AttentionKind, TranslationKey> = {
    pick_version: 'page.overview.attnPickVersion',
    missing_mods: 'page.overview.attnMissingMods',
    incompatible: 'page.overview.attnIncompatible',
    integrity: 'page.overview.attnIntegrity',
    modpack_update: 'page.overview.attnModpackUpdate',
  };

  const ICON_KEY: Record<AttentionKind, IconName> = {
    pick_version: 'warning',
    missing_mods: 'warning',
    incompatible: 'warning',
    integrity: 'warning',
    modpack_update: 'update',
  };
</script>

{#if items.length > 0}
  <div
    class="rounded-xl border border-warning-text bg-warning-bg overflow-hidden"
    data-testid="overview-attention"
  >
    <div
      class="px-4 py-2.5 font-semibold text-warning-text border-b border-warning-text
        flex items-center gap-2"
    >
      <Icon name="warning" class="text-warning-text" />
      {$t('page.overview.attentionHeading')}
    </div>
    {#each items as item (item.kind)}
      <button
        type="button"
        class="w-full px-4 py-2.5 flex items-center gap-3 text-left
          border-b border-warning-text/30 last:border-b-0 hover:bg-warning-text/10"
        data-testid="overview-attention-{item.kind}"
        onclick={() => onAction(item.kind)}
      >
        <Icon name={ICON_KEY[item.kind]} class="text-warning-text" />
        <span class="flex-1 text-sm text-warning-text">
          {$t(TEXT_KEY[item.kind], { count: item.count })}
        </span>
        <span class="inline-flex items-center gap-1 text-xs text-warning-text underline">
          {$t('page.overview.attentionView')}
          <Icon name="arrowRight" size={12} />
        </span>
      </button>
    {/each}
  </div>
{/if}
