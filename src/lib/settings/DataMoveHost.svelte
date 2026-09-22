<script lang="ts">
  // App-level owner of the data-folder move dialog. Mounted once, last, from +page.svelte and
  // driven entirely by the `dataLocation` store, so the dialog comes back after a reload (F5 is a
  // supported action) or a tab switch — the final state is the only way to restart, and used to
  // be local state of StoragePanel. StoragePanel only plans, confirms and starts a move.
  //
  // The reactive triggers live here rather than in the store (which stays a directly-callable
  // state machine): subscribe on mount, poll while running.
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import { commands } from '$lib/ipc/bindings';
  import { describeStoreError, formatError } from '$lib/ipc/format-error';
  import { pushInfo } from '$lib/toasts/toasts.svelte';
  import DataLocationProgressDialog from './DataLocationProgressDialog.svelte';
  import { dataLocation } from './data-location.svelte';
  import { restartLauncherOrExplain } from './restart';

  /** `get_data_location` is sync and state-only, so this is cheap. There is no state-change
   *  event, and a reloaded page has nobody awaiting the command — without the poll its dialog
   *  would never learn that the move ended. */
  const POLL_MS = 1500;
  /** How often to ask again while NO status read has succeeded yet. Until one does, "could not
   *  tell" must not settle as "no move": the backend may be sitting in `restart_required`,
   *  refusing every launch, and this dialog is the only way to restart. */
  const FIRST_READ_RETRY_MS = 3000;

  let cancelling = $state(false);
  let retrying = $state(false);
  let restarting = $state(false);
  let actionError = $state<string | null>(null);
  let restartError = $state<string | null>(null);

  const view = $derived(dataLocation.relocation);
  const active = $derived(view.kind === 'idle' ? null : view);
  // A primitive on purpose: `view` is a fresh object on every progress tick, and an effect keyed
  // on it would restart the interval before it ever fired.
  const running = $derived(view.kind === 'running');

  onMount(() => {
    const detach = dataLocation.attach();
    void dataLocation.init();
    // `detach` reads no $state — teardown reads are stale.
    return detach;
  });

  $effect(() => {
    if (!running) return;
    const id = setInterval(() => void dataLocation.refresh(), POLL_MS);
    return () => clearInterval(id);
  });

  $effect(() => {
    if (dataLocation.loaded) return;
    // `init()`, not `refresh()`: it shares a read that is already in flight.
    const id = setInterval(() => void dataLocation.init(), FIRST_READ_RETRY_MS);
    return () => clearInterval(id);
  });

  $effect(() => {
    if (running) return;
    // A pending cancel request and its error belong to a run that is over.
    cancelling = false;
    actionError = null;
  });

  $effect(() => {
    if (!dataLocation.orphanEnded) return;
    pushInfo(
      $t('settings.storage.dataLocation.orphanEnded', {
        path: dataLocation.status?.effective ?? '',
      }),
    );
    dataLocation.ackOrphanEnded();
  });

  async function cancel(): Promise<void> {
    if (cancelling) return;
    cancelling = true;
    actionError = null;
    try {
      // Not wrapped in typedError (it returns nothing), so a failure is a rejection. On success
      // `cancelling` stays set until the run leaves `running`: the command only REQUESTS a
      // cancel; the move itself reports how it ended.
      await commands.cancelDataLocationMove();
    } catch (e) {
      actionError = $t('settings.storage.dataLocation.progress.cancelFailed', {
        error: describeStoreError(e),
      });
      cancelling = false;
    }
  }

  async function retry(): Promise<void> {
    if (retrying || restarting) return;
    retrying = true;
    actionError = null;
    try {
      // The backend answers with the whole new status — including whether another retry can still
      // help (`retry_possible`) — so the dialog needs no second read and guesses nothing.
      const r = await commands.retryDataMoveCleanup();
      if (r.status === 'ok') dataLocation.applyRelocation(r.data);
      else
        actionError = $t('settings.storage.dataLocation.final.retryFailed', {
          error: formatError(r.error),
        });
    } catch (e) {
      // typedError rethrows real Error instances; the leftovers stay listed either way.
      actionError = $t('settings.storage.dataLocation.final.retryFailed', {
        error: describeStoreError(e),
      });
    } finally {
      retrying = false;
    }
  }

  async function openFolder(): Promise<void> {
    actionError = null;
    try {
      const r = await commands.openDataMoveLeftovers();
      if (r.status === 'error') {
        actionError = $t('settings.storage.dataLocation.final.openFolderFailed', {
          error: formatError(r.error),
        });
      }
    } catch (e) {
      actionError = $t('settings.storage.dataLocation.final.openFolderFailed', {
        error: describeStoreError(e),
      });
    }
  }

  async function restart(): Promise<void> {
    if (restarting || retrying) return;
    restarting = true;
    restartError = null;
    try {
      // On success this never returns: the process is replaced. Shared with the recovery-session
      // banner: in both places Restart is the way forward, so a failure ends in the way out.
      restartError = await restartLauncherOrExplain($t);
    } finally {
      restarting = false;
    }
  }
</script>

{#if active}
  <DataLocationProgressDialog
    view={active}
    {cancelling}
    {retrying}
    {restarting}
    {actionError}
    {restartError}
    onCancel={() => void cancel()}
    onRetry={() => void retry()}
    onOpenFolder={() => void openFolder()}
    onRestart={() => void restart()}
  />
{/if}
