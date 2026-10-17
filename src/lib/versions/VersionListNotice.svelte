<script lang="ts">
  // Under a Minecraft version picker that shows no version: why, said as what it is. The list is
  // the page's (`createMcVersions`): loading on first run, failed (the Overview's sentence, with
  // Reload, since reopening the dialog or the tab loads nothing), loaded but empty, or holding
  // only snapshots. The host renders this only while its picker shows no version.
  //
  // The keyboard user keeps a place: while a reload runs, the failure and its Reload stay (the
  // button says it is busy instead of leaving), and when a reload succeeds and this goes away,
  // the focus moves on to the picker that now has versions (`pickerId`).
  import { t } from '$lib/i18n';
  import Spinner from '$lib/ui/Spinner.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';

  let {
    total,
    loading,
    failure,
    onReload,
    pickerId,
  }: {
    /** Versions in the list, snapshots included. */
    total: number;
    loading: boolean;
    /** The last load's failure, `null` once a load succeeded. */
    failure: string | null;
    onReload: () => void;
    /** The version picker's id: the focus goes there when a reload empties this notice. */
    pickerId: string;
  } = $props();

  // Reload holds the focus: set when it gets it, cleared only when the user moves it to another
  // control (a blur with a target). Removing the node blurs it with none, so the flag survives that.
  let reloadFocused = false;

  $effect(() => () => {
    if (!reloadFocused) return;
    // Torn down with the focus on Reload (the reload filled the list): the focus goes to the
    // picker — not to <body>, nor to the first field a dialog's trap falls back to. Only while the
    // picker is still on the page: closing the whole dialog is not this case.
    queueMicrotask(() => {
      const picker = document.getElementById(pickerId);
      if (picker?.isConnected) picker.focus();
    });
  });
</script>

{#if total === 0 && failure !== null}
  <div class="mb-3 flex items-start gap-2" data-testid="version-list-failed">
    <StatusMessage tone="danger" message={failure} class="min-w-0 flex-1 text-xs" />
    <button
      type="button"
      class="btn-secondary btn-xs inline-flex flex-shrink-0 items-center gap-1"
      aria-disabled={loading}
      aria-busy={loading}
      onfocus={() => (reloadFocused = true)}
      onblur={(e) => {
        if (e.relatedTarget !== null) reloadFocused = false;
      }}
      onclick={() => {
        if (!loading) onReload();
      }}
    >
      {#if loading}<Spinner size="sm" />{/if}
      {$t('page.overview.errorRetry')}
    </button>
  </div>
{:else if total === 0 && loading}
  <div class="mb-3 text-secondary" data-testid="version-list-loading">
    <Spinner
      size="sm"
      labelPlacement="right"
      label={$t('instance.loader.loadingVersions')}
      delayMs={150}
    />
  </div>
{:else}
  <p class="mb-3 text-xs text-warning-text">
    {total === 0
      ? $t('instance.manage.noVersions')
      : $t('instance.manage.noReleasesEnableSnapshots')}
  </p>
{/if}
