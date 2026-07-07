<script lang="ts">
  import { commands, events, type Screenshot } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import ScreenshotGrid from './ScreenshotGrid.svelte';

  let { instanceId }: { instanceId: string | null } = $props();

  let shots = $state<Screenshot[]>([]);
  let listError = $state<string | null>(null);
  let loading = $state(false);

  async function reload() {
    if (!instanceId) {
      shots = [];
      return;
    }
    const reqId = instanceId;
    loading = true;
    listError = null;
    const r = await commands.listScreenshots(reqId);
    // Guard against a stale response arriving after the instance changed.
    if (instanceId !== reqId) return;
    loading = false;
    if (r.status === 'ok') shots = r.data;
    else listError = formatError(r.error);
  }

  $effect(() => {
    void instanceId;
    void reload();
  });

  // New screenshots appear once a play session ends.
  $effect(() => {
    let unlisten: (() => void) | null = null;
    void events.processExited.listen(() => void reload()).then((u) => (unlisten = u));
    return () => {
      if (unlisten) unlisten();
    };
  });

  async function openFolder() {
    if (!instanceId) return;
    await commands.openScreenshotsFolder(instanceId);
  }
</script>

{#if !instanceId}
  <p class="p-4 text-sm text-muted">{$t('screenshots.noInstance')}</p>
{:else if loading}
  <LoadingPanel label={$t('screenshots.loading')} />
{:else if listError}
  <p class="p-4 text-sm text-danger">{listError}</p>
{:else if shots.length === 0}
  <div class="flex flex-col items-center gap-3 p-8 text-center">
    <p class="text-sm text-muted">{$t('screenshots.emptyTab')}</p>
    <button
      type="button"
      class="btn-secondary btn-sm flex items-center gap-1.5"
      onclick={openFolder}
    >
      <Icon name="folderOpen" size={14} />
      {$t('screenshots.openFolder')}
    </button>
  </div>
{:else}
  <div class="flex items-center justify-end px-3 pt-3">
    <button
      type="button"
      class="btn-secondary btn-xs flex items-center gap-1.5"
      onclick={openFolder}
    >
      <Icon name="folderOpen" size={14} />
      {$t('screenshots.openFolder')}
    </button>
  </div>
  <ScreenshotGrid {shots} onChanged={reload} />
{/if}
