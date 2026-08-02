<script lang="ts">
  // The AI pre-fill's confirm-and-watch dialog: estimate → progress → summary.
  //
  // Stacked on top of LocalizationModal, so it is rendered AFTER it (Modal's
  // Escape stack is mount-ordered — see Modal.svelte's module doc) and it
  // deliberately mounts neither a `Select` nor a second "close"-named button:
  // LocalizationModal's suite queries `getByRole('combobox')` and
  // `getByRole('button', { name: /close/i })` in the singular.
  //
  // The estimate is fetched here rather than passed in because it is the
  // dialog's whole reason to exist: nothing is spent until the user has seen
  // a number and pressed Translate.
  //
  // The `Channel` lives in prefill-runner.ts, not here — see its doc comment.
  import { onMount } from 'svelte';
  import { commands, type PrefillEstimate, type PrefillProgress } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import DialogTitle from '$lib/ui/DialogTitle.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { runPrefill } from './prefill-runner';
  import { summaryView, type SummaryView } from './prefill';

  let {
    instanceId,
    lang,
    namespace = null,
    onClose,
    onFinished,
  }: {
    instanceId: string;
    lang: string;
    /** One resource namespace, or `null` for the whole instance. */
    namespace?: string | null;
    onClose: () => void;
    /** Fired once a run produced a summary — including a cancelled or a
     *  partially-failed one, both of which still wrote strings the coverage
     *  percentages need to catch up with. */
    onFinished?: () => void;
  } = $props();

  type Phase = 'estimating' | 'estimate' | 'running' | 'summary';

  let phase = $state<Phase>('estimating');
  let estimate = $state<PrefillEstimate | null>(null);
  let estimateError = $state<string | null>(null);
  let progress = $state<PrefillProgress | null>(null);
  let summary = $state<SummaryView | null>(null);
  let runError = $state<string | null>(null);
  let stopping = $state(false);

  const titleId = `l10n-prefill-title-${crypto.randomUUID()}`;

  onMount(async () => {
    const res = await commands.l10nPrefillEstimate(instanceId, lang, namespace);
    if (res.status === 'ok') {
      estimate = res.data;
      phase = 'estimate';
    } else {
      // The estimate shares the run's preflight, so a missing API key or an
      // unset local model name surfaces here — before the user has confirmed
      // anything — rather than after.
      estimateError = $t('instance.l10n.prefill.estimateFailed', {
        error: formatError(res.error),
      });
      phase = 'estimate';
    }
  });

  // A stable lowercase token from the backend. Unknown phases fall back to a
  // generic label rather than rendering the raw token: the set is the
  // backend's to grow, and a user should never see `some_new_phase`.
  const PHASE_LABELS: Record<string, string> = $derived({
    scanning: $t('instance.l10n.prefill.phaseScanning'),
    free: $t('instance.l10n.prefill.phaseFree'),
    name: $t('instance.l10n.prefill.phaseName'),
    prose: $t('instance.l10n.prefill.phaseProse'),
    other: $t('instance.l10n.prefill.phaseOther'),
    applying: $t('instance.l10n.prefill.phaseApplying'),
  });
  const phaseLabel = $derived(
    progress ? (PHASE_LABELS[progress.phase] ?? $t('instance.l10n.prefill.phaseWorking')) : '',
  );

  const scopeLabel = $derived(
    namespace === null
      ? $t('instance.l10n.prefill.scopeAll')
      : $t('instance.l10n.prefill.scopeNamespace', { namespace }),
  );

  async function start() {
    if (phase !== 'estimate') return;
    phase = 'running';
    runError = null;
    progress = null;
    const outcome = await runPrefill(instanceId, lang, namespace, (p) => {
      progress = p;
    });
    if (outcome.status === 'ok') {
      summary = summaryView(outcome.summary);
      phase = 'summary';
      onFinished?.();
    } else {
      // No summary at all — the run never got far enough to buy anything
      // (busy, consent off, no stored key). Back to the estimate so the user
      // can fix it and retry without reopening the dialog.
      runError = $t('instance.l10n.prefill.startFailed', { error: outcome.message });
      phase = 'estimate';
    }
    stopping = false;
  }

  async function stop() {
    if (stopping) return;
    stopping = true;
    // Fire-and-forget by design: the command only flips a flag the run polls
    // between batches, and the run itself still returns its summary.
    await commands.l10nPrefillCancel(instanceId);
  }
</script>

<Modal
  ariaLabelledby={titleId}
  {onClose}
  closeOnBackdrop={phase !== 'running'}
  closeOnEscape={phase !== 'running'}
  panelClass="w-[480px] max-w-full p-5 flex flex-col gap-3"
  dataTestid="l10n-prefill-dialog"
>
  <DialogTitle id={titleId}>
    {#if phase === 'summary' && summary}
      {summary.cancelled
        ? $t('instance.l10n.prefill.summaryCancelledTitle')
        : $t('instance.l10n.prefill.summaryTitle')}
    {:else}
      {$t('instance.l10n.prefill.title')}
    {/if}
  </DialogTitle>

  <p class="text-sm text-secondary">{scopeLabel}</p>

  {#if phase === 'estimating'}
    <LoadingPanel label={$t('instance.l10n.prefill.estimating')} />
  {:else if phase === 'estimate'}
    {#if estimateError}
      <p class="text-sm text-danger" data-testid="l10n-prefill-error">{estimateError}</p>
    {:else if estimate}
      <dl class="flex flex-col gap-1 text-sm" data-testid="l10n-prefill-estimate">
        <div class="flex items-baseline justify-between gap-3">
          <dt class="text-secondary">{$t('instance.l10n.prefill.missingLabel')}</dt>
          <dd class="font-mono text-primary" data-testid="l10n-prefill-missing">
            {estimate.missing}
          </dd>
        </div>
        <div class="flex items-baseline justify-between gap-3">
          <dt class="text-secondary">{$t('instance.l10n.prefill.fromCacheLabel')}</dt>
          <dd class="font-mono text-primary" data-testid="l10n-prefill-from-cache">
            {estimate.fromCache}
          </dd>
        </div>
        <div class="flex items-baseline justify-between gap-3">
          <dt class="text-secondary">{$t('instance.l10n.prefill.fromGlossaryLabel')}</dt>
          <dd class="font-mono text-primary" data-testid="l10n-prefill-from-glossary">
            {estimate.fromGlossary}
          </dd>
        </div>
        <div class="flex items-baseline justify-between gap-3">
          <dt class="font-medium text-primary">{$t('instance.l10n.prefill.toTranslateLabel')}</dt>
          <dd class="font-mono font-medium text-primary" data-testid="l10n-prefill-to-translate">
            {estimate.toTranslate}
          </dd>
        </div>
        <div class="flex items-baseline justify-between gap-3">
          <dt class="text-secondary">{$t('instance.l10n.prefill.tokensLabel')}</dt>
          <dd class="font-mono text-primary" data-testid="l10n-prefill-tokens">
            {estimate.estimatedTokens}
          </dd>
        </div>
      </dl>
      <p class="text-xs text-muted">{$t('instance.l10n.prefill.tokensNote')}</p>
      <p class="text-xs text-muted" data-testid="l10n-prefill-assets-note">
        {$t('instance.l10n.prefill.assetsNote')}
      </p>
      {#if estimate.provider === 'local'}
        <p class="text-xs text-warning-text" data-testid="l10n-prefill-local-note">
          {$t('instance.l10n.prefill.localNote')}
        </p>
      {/if}
      {#if estimate.billable}
        <p class="text-xs text-warning-text" data-testid="l10n-prefill-billable-note">
          {$t('instance.l10n.prefill.billableNote')}
        </p>
      {/if}
    {/if}
    {#if runError}
      <p class="text-sm text-danger" data-testid="l10n-prefill-error">{runError}</p>
    {/if}
  {:else if phase === 'running'}
    <div class="flex flex-col gap-1" data-testid="l10n-prefill-progress">
      <span class="text-sm text-primary">
        {progress
          ? $t('instance.l10n.prefill.progressLabel', {
              done: progress.done,
              total: progress.total,
            })
          : $t('instance.l10n.prefill.phaseWorking')}
      </span>
      {#if phaseLabel}
        <span class="text-xs text-muted">{phaseLabel}</span>
      {/if}
    </div>
  {:else if phase === 'summary' && summary}
    <dl class="flex flex-col gap-1 text-sm" data-testid="l10n-prefill-summary">
      <div class="flex items-baseline justify-between gap-3">
        <dt class="font-medium text-primary">{$t('instance.l10n.prefill.writtenLabel')}</dt>
        <dd class="font-mono font-medium text-primary" data-testid="l10n-prefill-written">
          {summary.written}
        </dd>
      </div>
      <div class="flex items-baseline justify-between gap-3">
        <dt class="text-secondary">{$t('instance.l10n.prefill.cachedLabel')}</dt>
        <dd class="font-mono text-primary" data-testid="l10n-prefill-cached">
          {summary.fromCache}
        </dd>
      </div>
      <div class="flex items-baseline justify-between gap-3">
        <dt class="text-secondary">{$t('instance.l10n.prefill.glossaryLabel')}</dt>
        <dd class="font-mono text-primary" data-testid="l10n-prefill-glossary">
          {summary.fromGlossary}
        </dd>
      </div>
      <div class="flex items-baseline justify-between gap-3">
        <dt class="text-secondary">{$t('instance.l10n.prefill.rejectedLabel')}</dt>
        <dd class="font-mono text-primary" data-testid="l10n-prefill-rejected">
          {summary.rejected}
        </dd>
      </div>
      <!--
        Rendered ONLY when the provider reported usage. A local model reports
        none, and a zero here would claim the run was free rather than that
        its cost is unknown — two different statements.
      -->
      {#if summary.tokens}
        <div class="flex items-baseline justify-between gap-3">
          <dt class="text-secondary">{$t('instance.l10n.prefill.costLabel')}</dt>
          <dd class="font-mono text-primary" data-testid="l10n-prefill-cost">
            {$t('instance.l10n.prefill.costValue', {
              prompt: summary.tokens.prompt,
              completion: summary.tokens.completion,
            })}
          </dd>
        </div>
      {/if}
    </dl>
    {#if summary.rejected > 0}
      <p class="text-xs text-muted">{$t('instance.l10n.prefill.rejectedNote')}</p>
    {/if}
    <!--
      Both caveats sit BESIDE the counts, never instead of them: everything
      counted above was verified and (for a cloud provider) paid for, and the
      strings are on disk either way.
    -->
    {#if summary.failed !== null}
      <div
        class="rounded border border-warning-text/40 bg-warning-bg p-3 text-xs text-warning-text"
      >
        <p class="font-medium">{$t('instance.l10n.prefill.failedTitle')}</p>
        <p data-testid="l10n-prefill-failed">
          {$t('instance.l10n.prefill.failedBody', { error: summary.failed })}
        </p>
      </div>
    {/if}
    {#if summary.packError}
      <div
        class="rounded border border-warning-text/40 bg-warning-bg p-3 text-xs text-warning-text"
      >
        <p class="font-medium">{$t('instance.l10n.prefill.packFailedTitle')}</p>
        <p data-testid="l10n-prefill-pack-warning">
          {$t('instance.l10n.prefill.packFailedBody', {
            error: summary.packError.message ?? $t('instance.l10n.prefill.packFailedUnknown'),
          })}
        </p>
      </div>
    {/if}
  {/if}

  <div class="mt-2 flex justify-end gap-2">
    {#if phase === 'summary'}
      <button
        type="button"
        class="btn-primary btn-sm"
        data-testid="l10n-prefill-done"
        onclick={onClose}
      >
        {$t('instance.l10n.prefill.doneButton')}
      </button>
    {:else if phase === 'running'}
      <BusyButton
        busy={stopping}
        class="btn-secondary btn-sm"
        data-testid="l10n-prefill-stop"
        onclick={stop}
      >
        {$t('instance.l10n.prefill.stopButton')}
      </BusyButton>
    {:else}
      <button
        type="button"
        class="btn-ghost btn-sm"
        data-testid="l10n-prefill-cancel"
        onclick={onClose}
      >
        {$t('common.cancel')}
      </button>
      <button
        type="button"
        class="btn-primary btn-sm"
        disabled={estimate === null}
        data-testid="l10n-prefill-start"
        onclick={start}
      >
        {$t('instance.l10n.prefill.startButton')}
      </button>
    {/if}
  </div>
</Modal>
