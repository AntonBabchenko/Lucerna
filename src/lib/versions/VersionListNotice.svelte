<script lang="ts">
  // Under a Minecraft version picker that shows no version: why, said as what it is. The list is
  // the page's (`createMcVersions`): loading on first run, failed (the Overview's sentence, with
  // Reload, since reopening the dialog or the tab loads nothing), loaded but empty, or holding
  // only snapshots. The host renders this only while its picker shows no version.
  import { t } from '$lib/i18n';
  import Spinner from '$lib/ui/Spinner.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';

  let {
    total,
    loading,
    failure,
    onReload,
  }: {
    /** Versions in the list, snapshots included. */
    total: number;
    loading: boolean;
    /** The last load's failure, `null` once a load succeeded. */
    failure: string | null;
    onReload: () => void;
  } = $props();
</script>

{#if total === 0 && loading}
  <div class="mb-3 text-secondary" data-testid="version-list-loading">
    <Spinner
      size="sm"
      labelPlacement="right"
      label={$t('instance.loader.loadingVersions')}
      delayMs={150}
    />
  </div>
{:else if total === 0 && failure !== null}
  <div class="mb-3 flex items-start gap-2" data-testid="version-list-failed">
    <StatusMessage tone="danger" message={failure} class="min-w-0 flex-1 text-xs" />
    <button type="button" class="btn-secondary btn-xs flex-shrink-0" onclick={onReload}>
      {$t('page.overview.errorRetry')}
    </button>
  </div>
{:else}
  <p class="mb-3 text-xs text-warning-text">
    {total === 0
      ? $t('instance.manage.noVersions')
      : $t('instance.manage.noReleasesEnableSnapshots')}
  </p>
{/if}
