<script lang="ts">
  import { t } from '$lib/i18n';

  // A thin bar pinned above the list, visible ONLY when there are dependency
  // problems or available updates. Each segment activates the corresponding
  // quick-filter on click (spec §4.3). Hidden entirely when the list is clean.
  let {
    issues,
    updates,
    onShowIssues,
    onShowUpdates,
  }: {
    issues: number;
    updates: number;
    onShowIssues: () => void;
    onShowUpdates: () => void;
  } = $props();
</script>

{#if issues > 0 || updates > 0}
  <div
    class="sticky top-0 z-10 flex items-center gap-3 px-3 py-1.5 text-xs rounded mb-2 bg-warning-bg/60 border border-warning-text/20"
    data-testid="attention-bar"
  >
    {#if issues > 0}
      <button type="button" class="text-danger hover:underline" onclick={onShowIssues}>
        {$t('mods.installed.attentionIssues', { count: issues })}
      </button>
    {/if}
    {#if issues > 0 && updates > 0}<span class="text-muted">·</span>{/if}
    {#if updates > 0}
      <button type="button" class="text-warning-text hover:underline" onclick={onShowUpdates}>
        {$t('mods.installed.attentionUpdates', { count: updates })}
      </button>
    {/if}
  </div>
{/if}
