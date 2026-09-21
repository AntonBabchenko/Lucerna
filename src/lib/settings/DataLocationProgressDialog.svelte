<script lang="ts">
  // The data-folder move's blocking dialog: live progress while the move runs (copying →
  // verifying → switching → deleting), then — when the move switched folders but could not
  // finish tidying up — the final state, which holds the only way forward (Restart). A clean
  // move ends in `app.restart()`, so the running state is normally torn down by the restart.
  //
  // Presentational: every command lives in DataMoveHost.svelte, which renders this from the
  // `dataLocation` store so the dialog survives a reload (F5) and a tab switch. It cannot be
  // dismissed: while running the backend refuses everything else, and in the final state Restart
  // is the only exit. Cancel is offered while copying and verifying only — after that the switch
  // is a short transaction that must not be interrupted.
  //
  // Two verdicts are the BACKEND's and are only read here, never re-derived: `old_root_is_default`
  // (a canonical path compare — the default folder also holds data-location.json, so the dialog
  // must never say "delete that whole folder" about it) and `retry_possible` (some leftover is not
  // an entry the running launcher owns). The frontend compares no paths and knows no entry names.
  //
  // Focus (docs/DESIGN.md §8): `trapFocus` skips natively-disabled buttons and its Tab handler
  // lives on the panel, so a FOCUSED control that turns `disabled` drops focus to <body> and Tab
  // starts walking the page behind the dialog. BusyButton disables natively while busy, so every
  // handler parks focus on the body wrapper BEFORE telling the host (which flips the flag); the
  // plain button uses `aria-disabled` + a click guard. Focus goes to Restart when the final state
  // appears and when a retry or a failed restart settles.
  import { tick } from 'svelte';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import type { MovePhase } from '$lib/ipc/bindings';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import DialogTitle from '$lib/ui/DialogTitle.svelte';
  import { Icon } from '$lib/ui/icons';
  import Modal from '$lib/ui/Modal.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import type { RelocationView } from './data-location.svelte';

  let {
    view,
    cancelling = false,
    retrying = false,
    restarting = false,
    actionError = null,
    restartError = null,
    onCancel,
    onRetry,
    onOpenFolder,
    onRestart,
  }: {
    view: Exclude<RelocationView, { kind: 'idle' }>;
    cancelling?: boolean;
    retrying?: boolean;
    restarting?: boolean;
    /** A failed cancel, retry or open-folder — already localized. */
    actionError?: string | null;
    /** A failed restart — already localized, including the way out. */
    restartError?: string | null;
    onCancel: () => void;
    onRetry: () => void;
    onOpenFolder: () => void;
    onRestart: () => void;
  } = $props();

  // A Record, so a new MovePhase member is a compile error here.
  const PHASE_KEYS: Record<MovePhase, TranslationKey> = {
    copying: 'settings.storage.dataLocation.progress.copying',
    verifying: 'settings.storage.dataLocation.progress.verifying',
    switching: 'settings.storage.dataLocation.progress.switching',
    deleting: 'settings.storage.dataLocation.progress.deleting',
  };

  const titleId = `data-move-title-${crypto.randomUUID()}`;
  let bodyEl = $state<HTMLDivElement | null>(null);

  const running = $derived(view.kind === 'running' ? view : null);
  const final = $derived(view.kind === 'restart_required' ? view : null);
  const details = $derived(final?.details ?? null);
  const phase = $derived(running?.phase ?? null);
  const phaseText = $derived(
    $t(phase === null ? 'settings.storage.dataLocation.progress.preparing' : PHASE_KEYS[phase]),
  );
  const canCancel = $derived(phase === 'copying' || phase === 'verifying');
  const pastCancel = $derived(phase === 'switching' || phase === 'deleting');
  // Bytes only mean something while copying; null = no bar.
  const percent = $derived.by(() => {
    const p = running?.progress;
    if (!p || phase !== 'copying') return null;
    const total = p.total_bytes ?? 0;
    if (total <= 0) return null;
    return Math.min(100, Math.round(((p.copied_bytes ?? 0) / total) * 100));
  });

  // Unknown details resolve to the restrictive answers: no "delete the whole folder", no retry.
  const oldIsDefault = $derived(details?.old_root_is_default ?? false);
  const canRetry = $derived(details?.retry_possible ?? false);
  const actionBusy = $derived(retrying || restarting);

  function parkFocus(): void {
    bodyEl?.focus();
  }

  async function focusRestart(): Promise<void> {
    await tick();
    bodyEl?.querySelector<HTMLElement>('[data-testid="data-move-restart"]')?.focus();
  }

  $effect(() => {
    if (final) void focusRestart();
  });

  // Plain variable: an effect-local memory, not state anyone renders.
  let wasBusy = false;
  $effect(() => {
    const busyNow = actionBusy;
    if (wasBusy && !busyNow) void focusRestart();
    wasBusy = busyNow;
  });

  function cancelClick(): void {
    if (cancelling) return;
    parkFocus();
    onCancel();
  }

  function retryClick(): void {
    if (actionBusy) return;
    parkFocus();
    onRetry();
  }

  function restartClick(): void {
    if (actionBusy) return;
    parkFocus();
    onRestart();
  }

  function openFolderClick(): void {
    // aria-disabled, not disabled: the button keeps focus, so it needs a guard.
    if (actionBusy) return;
    onOpenFolder();
  }
</script>

<Modal
  ariaLabelledby={titleId}
  onClose={() => {}}
  closeOnBackdrop={false}
  closeOnEscape={false}
  panelClass="w-[480px] max-w-full p-5"
  dataTestid="data-move-dialog"
>
  <div
    bind:this={bodyEl}
    tabindex="-1"
    class="flex flex-col gap-3 outline-none"
    data-testid="data-move-body"
  >
    <DialogTitle id={titleId}>
      {final
        ? $t('settings.storage.dataLocation.final.title')
        : $t('settings.storage.dataLocation.progress.title')}
    </DialogTitle>

    {#if running}
      <div class="flex items-center gap-2 text-sm text-secondary">
        <Spinner size="sm" label={phaseText} />
        <span aria-hidden="true" data-testid="data-move-phase">{phaseText}</span>
      </div>
      {#if percent !== null}
        <div class="flex items-center gap-2">
          <div
            role="progressbar"
            aria-label={$t('settings.storage.dataLocation.progress.barLabel')}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={percent}
            class="h-2 flex-1 bg-subtle rounded overflow-hidden"
          >
            <div
              class="h-full bg-accent"
              style="width: {percent}%"
              data-testid="data-location-progress-bar"
            ></div>
          </div>
          <span class="w-10 text-right text-xs text-muted tabular-nums" aria-hidden="true"
            >{percent}%</span
          >
        </div>
      {/if}
      <StatusMessage
        message={cancelling && pastCancel
          ? $t('settings.storage.dataLocation.progress.cancelTooLate')
          : null}
        tone="info"
      />
      <StatusMessage message={actionError} tone="danger" />
      {#if canCancel}
        <div class="flex justify-end">
          <BusyButton
            busy={cancelling}
            class="btn-secondary btn-sm"
            data-testid="data-move-cancel"
            onclick={cancelClick}
          >
            {$t('settings.storage.dataLocation.progress.cancelBtn')}
          </BusyButton>
        </div>
      {/if}
    {:else if final}
      {#if details === null}
        <p class="text-sm text-secondary">
          {$t('settings.storage.dataLocation.final.detailsUnknown')}
        </p>
      {:else}
        <p class="text-sm text-secondary selectable">
          {$t('settings.storage.dataLocation.final.movedTo', { path: details.new_root })}
        </p>
        {#if details.old_root_intact}
          <p class="text-sm text-secondary selectable">
            {$t(
              oldIsDefault
                ? 'settings.storage.dataLocation.final.intactDefaultDir'
                : 'settings.storage.dataLocation.final.intact',
              { path: details.old_root },
            )}
          </p>
        {:else if details.leftovers.length > 0}
          <p class="text-sm text-secondary selectable">
            {$t('settings.storage.dataLocation.final.leftoversIntro', {
              count: details.leftovers.length,
              path: details.old_root,
            })}
          </p>
          <ul
            class="max-h-32 overflow-y-auto list-disc pl-5 font-mono text-xs text-secondary selectable"
            data-testid="data-move-leftovers"
          >
            {#each details.leftovers as name (name)}
              <li>{name}</li>
            {/each}
          </ul>
          {#if !details.retry_possible}
            <!-- Only here is the promise true: the old folder is not intact and the backend found
                 nothing a retry could remove, so every listed name is the running launcher's own. -->
            <p class="text-xs text-muted" data-testid="data-move-own-leftovers">
              {$t('settings.storage.dataLocation.final.ownLeftoversNote')}
            </p>
          {/if}
        {:else}
          <p class="text-sm text-secondary selectable">
            {$t('settings.storage.dataLocation.final.cleaned', { path: details.old_root })}
          </p>
        {/if}
        {#if oldIsDefault}
          <p class="text-sm text-secondary font-medium" data-testid="data-move-keep-redirect">
            {$t('settings.storage.dataLocation.keepRedirectNote')}
          </p>
        {/if}
      {/if}
      <p class="text-sm text-secondary font-medium">
        {$t('settings.storage.dataLocation.final.restartHint')}
      </p>
      <StatusMessage message={actionError} tone="danger" />
      <StatusMessage message={restartError} tone="danger" />
      <div class="mt-2 flex flex-wrap justify-end gap-2">
        {#if details !== null}
          <button
            type="button"
            class="btn-secondary btn-sm inline-flex items-center gap-1.5"
            aria-disabled={actionBusy}
            data-testid="data-move-open-folder"
            onclick={openFolderClick}
          >
            <Icon name="folderOpen" size={14} />
            {$t('settings.storage.dataLocation.final.openFolderBtn')}
          </button>
        {/if}
        {#if canRetry}
          <BusyButton
            busy={retrying}
            disabled={restarting}
            class="btn-secondary btn-sm"
            data-testid="data-move-retry"
            onclick={retryClick}
          >
            {$t('settings.storage.dataLocation.final.retryBtn')}
          </BusyButton>
        {/if}
        <BusyButton
          busy={restarting}
          disabled={retrying}
          class="btn-primary btn-sm"
          data-testid="data-move-restart"
          data-autofocus
          onclick={restartClick}
        >
          {$t('settings.storage.dataLocation.final.restartBtn')}
        </BusyButton>
      </div>
    {/if}
  </div>
</Modal>
