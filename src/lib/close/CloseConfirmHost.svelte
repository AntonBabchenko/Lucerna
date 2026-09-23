<script lang="ts">
  /*
    The window's close asks before it kills anything (batch 11c).

    The backend owns the close: it holds the window open, looks at what closing
    would lose, and sends `closeConfirmNeeded` when something would be. This host
    shows that question and answers it. It never listens to
    `tauri://close-requested` itself — a page-side listener makes Tauri prevent
    EVERY close, and a crashed page would leave the app unclosable.

    `emit` succeeding does not mean anyone saw the question, so once the dialog
    is on screen the host says so (`appCloseAskShown`). If it does not within
    the backend's budget, the native dialog asks instead and the late ack comes
    back `false` — this copy then closes, so the user never sees two.

    One slot, not a list: a later ask (another ×, or a re-prompt after the state
    grew) REPLACES the open one.
  */
  import { onDestroy, onMount } from 'svelte';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import { t } from '$lib/i18n';
  import { commands, events, type CloseLosses } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { closeConfirmLabel, closeConfirmVariant, closeLines } from './close-copy';
  import { closeAskState } from './close-ask.svelte';

  type Ask = { generation: number; losses: CloseLosses };

  let ask = $state<Ask | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let unlisten: (() => void) | null = null;
  let destroyed = false;

  onMount(() => {
    events.closeConfirmNeeded
      .listen(({ payload }) => {
        ask = { generation: payload.generation, losses: payload.losses };
        busy = false;
        error = null;
      })
      .then((u) => {
        if (destroyed) u();
        else unlisten = u;
      });
  });

  onDestroy(() => {
    destroyed = true;
    unlisten?.();
    closeAskState.open = false;
  });

  $effect(() => {
    closeAskState.open = ask !== null;
  });

  // The ack, sent after the dialog for THIS generation has rendered. Effects
  // run after the DOM update, so the modal is mounted by the time this fires.
  $effect(() => {
    const current = ask;
    if (!current) return;
    const generation = current.generation;
    commands.appCloseAskShown(generation).then(
      (shown) => {
        if (!shown && ask?.generation === generation) ask = null;
      },
      () => {
        // The ack could not be sent, so the backend will ask natively. Close
        // this copy rather than leave two questions on screen.
        if (ask?.generation === generation) ask = null;
      },
    );
  });

  function cancel() {
    const current = ask;
    ask = null;
    if (!current) return;
    // Lets the backend drop the question, so the scheduled hide-to-tray may
    // run again. If this does not land, the only effect is that hide stays
    // held until the next close question — nothing is killed or kept open.
    commands.appCancelClose(current.generation).catch(() => {});
  }

  async function confirm() {
    const current = ask;
    if (!current || busy) return;
    busy = true;
    error = null;
    const result = await commands.appConfirmClose(current.generation, current.losses);
    // A re-prompt has replaced this ask: the new one is what is on screen.
    if (ask?.generation !== current.generation) return;
    if (result.status === 'error') {
      busy = false;
      error = formatError(result.error);
    }
    // Ok: either the app is exiting, or a re-prompt naming something new is on
    // its way and will replace this dialog. Stay busy until one happens; a
    // further × starts a fresh ask, which resets it.
  }
</script>

{#if ask}
  <ConfirmDialog
    title={$t('closeConfirm.title')}
    bodyText={closeLines(ask.losses, $t)}
    confirmLabel={closeConfirmLabel(ask.losses, $t)}
    variant={closeConfirmVariant(ask.losses)}
    {busy}
    {error}
    confirmTestid="close-confirm"
    onCancel={cancel}
    onConfirm={confirm}
  />
{/if}
