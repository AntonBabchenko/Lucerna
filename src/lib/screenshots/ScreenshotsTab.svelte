<script lang="ts">
  import { commands, events, type Screenshot } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { listenUntilDestroyed } from '$lib/ipc/listen';
  import { pushWarning } from '$lib/toasts/toasts.svelte';
  import { t } from '$lib/i18n';
  import { get } from 'svelte/store';
  import { Icon } from '$lib/ui/icons';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import ScreenshotBrowser from './ScreenshotBrowser.svelte';

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

  // New screenshots appear once a play session ends. Race-safe subscribe:
  // the old late-assigned-unlisten effect leaked the listener when the tab
  // unmounted before listen() resolved.
  listenUntilDestroyed([events.processExited.listen(() => void reload())]);

  async function openFolder() {
    if (!instanceId) return;
    const r = await commands.openScreenshotsFolder(instanceId);
    if (r.status !== 'ok') {
      pushWarning(get(t)('screenshots.openFolder'), [formatError(r.error)]);
    }
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
  <ScreenshotBrowser {shots} onChanged={reload} resetKey={instanceId} {controls} />
{/if}

{#snippet controls()}
  <button type="button" class="btn-secondary btn-xs flex items-center gap-1.5" onclick={openFolder}>
    <Icon name="folderOpen" size={14} />
    {$t('screenshots.openFolder')}
  </button>
{/snippet}
