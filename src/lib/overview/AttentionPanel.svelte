<script lang="ts">
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { Icon, type IconName } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import type { AttentionItem, AttentionKind } from './attention';

  let {
    items,
    onAction,
    onDismiss,
  }: {
    items: AttentionItem[];
    onAction: (kind: AttentionKind) => void;
    onDismiss: () => void;
  } = $props();

  const TEXT_KEY: Record<AttentionKind, TranslationKey> = {
    log_issue: 'page.overview.attnLogIssue',
    pick_version: 'page.overview.attnPickVersion',
    missing_mods: 'page.overview.attnMissingMods',
    incompatible: 'page.overview.attnIncompatible',
    integrity: 'page.overview.attnIntegrity',
    modpack_update: 'page.overview.attnModpackUpdate',
  };

  const ICON_KEY: Record<AttentionKind, IconName> = {
    log_issue: 'warning',
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
      <button
        type="button"
        class="btn-icon ml-auto -my-1 -mr-1 !text-warning-text hover:!bg-warning-text/10"
        aria-label={$t('page.overview.attentionDismissAria')}
        use:tooltip={$t('page.overview.attentionDismissTooltip')}
        onclick={onDismiss}
        data-testid="overview-attention-dismiss"><Icon name="close" size={16} /></button
      >
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
