<script lang="ts">
  // "Apply in the other instances too?" — offered right after an Apply or an
  // import.
  //
  // The override store is GLOBAL, so a translation typed once is usable by
  // every instance that ships the same mods; only the per-instance resource
  // pack is local. Without this dialog the user would have to remember that
  // and walk the instance list by hand, so the launcher makes the offer.
  //
  // Which instances are worth offering, and which can be ticked right now, is
  // the backend's call: `candidate` and `actionable` arrive decided on each
  // row (a running instance or one mid-pre-fill is a candidate you cannot
  // touch yet). This component reads those flags and never re-derives them —
  // a second opinion here would drift from the one the backend enforces.
  import { onMount } from 'svelte';
  import { commands, type ApplyTarget } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import DialogTitle from '$lib/ui/DialogTitle.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import Modal from '$lib/ui/Modal.svelte';

  let {
    lang,
    exclude = null,
    unsolicited = true,
    onClose,
  }: {
    lang: string;
    /** True when the launcher offered this on its own after an import or a
     *  pre-fill run, false when the user pressed the button for it.
     *
     *  It decides what "nothing to do" looks like. Vanishing without a word is
     *  right for an offer nobody asked for — over-triggering then costs
     *  nothing. It is wrong for a press: that is a dead click, and the user is
     *  left unsure whether anything happened. */
    unsolicited?: boolean;
    /** The instance that just applied — the post-Apply trigger passes it so
     *  the dialog does not offer the one the user is already looking at. The
     *  post-import trigger passes `null`: after an import the current
     *  instance's own pack is stale too. */
    exclude?: string | null;
    onClose: () => void;
  } = $props();

  const titleId = `l10n-apply-targets-title-${crypto.randomUUID()}`;

  /** 'applied' / 'deferred' are outcomes; anything else is a formatted error. */
  type RowResult = 'applied' | 'deferred' | string;

  let rows = $state<ApplyTarget[]>([]);
  let checked = $state<Record<string, boolean>>({});
  let results = $state<Record<string, RowResult>>({});
  let running = $state(false);
  let loaded = $state(false);
  /** Set once a run has completed. The secondary button then reads Close:
   *  offering to cancel work that has already been written is a lie. */
  let hasRun = $state(false);
  // There is no `open` prop to poll for dismissal mid-sequence (Modal is
  // mounted by the parent inside an `{#if}`), so record it locally. EVERY
  // close path goes through `closeSelf`, including Modal's own Escape and
  // backdrop handling — wiring Modal to the raw `onClose` would let those two
  // bypass the flag and the loop would keep applying to instances after the
  // user closed the dialog.
  let dismissed = false;

  function closeSelf() {
    dismissed = true;
    onClose();
  }

  onMount(async () => {
    const res = await commands.l10nApplyTargets(lang);
    if (dismissed) return;
    if (res.status !== 'ok') {
      // This dialog is offered proactively, not requested. Failing to
      // enumerate the other instances is not worth an error the user has to
      // dismiss for an action they never asked for — the apply they *did* ask
      // for already reported its own outcome.
      closeSelf();
      return;
    }
    const candidates = res.data.filter((row) => row.candidate && row.instanceId !== exclude);
    // A wall of disabled rows with nothing to do is worse than no dialog at
    // all — but only when the dialog appeared uninvited. A user who pressed
    // the button gets told, below, that there is nothing to carry.
    if (unsolicited && !candidates.some((row) => row.actionable)) {
      closeSelf();
      return;
    }
    rows = candidates;
    checked = Object.fromEntries(candidates.map((row) => [row.instanceId, row.actionable]));
    loaded = true;
  });

  const tickedCount = $derived(
    rows.filter((row) => row.actionable && checked[row.instanceId]).length,
  );

  function stateLabel(state: ApplyTarget['state']): string {
    if (state === 'not_applied') return $t('instance.l10n.targets.stateNotApplied');
    if (state === 'outdated') return $t('instance.l10n.targets.stateOutdated');
    // 'current' / 'not_applicable' are not candidates the backend offers; if
    // one ever arrives, show the row without inventing a label for it.
    return '';
  }

  function blockedReason(row: ApplyTarget): string {
    if (row.isRunning) return $t('instance.l10n.targets.running');
    if (row.prefillActive) return $t('instance.l10n.targets.prefill');
    return '';
  }

  function resultText(result: RowResult): string {
    if (result === 'applied') return $t('instance.l10n.targets.applied');
    if (result === 'deferred') return $t('instance.l10n.targets.deferred');
    return result;
  }

  function resultClass(result: RowResult): string {
    if (result === 'applied') return 'text-success';
    // Deferred is neither success nor failure: the pack IS written, it just
    // cannot flip on in options.txt until the instance has been launched
    // once. Blue info, the same tone the editor's deferred toast uses — never
    // the green that would claim it is live.
    if (result === 'deferred') return 'text-accent';
    return 'text-danger';
  }

  // Sequential on purpose: each apply rewrites one instance's pack and
  // touches its options.txt, and a per-row line lands as it goes so a slow
  // run reads as progress rather than a frozen dialog.
  async function run() {
    if (running) return;
    running = true;
    try {
      for (const row of rows) {
        if (!row.actionable || !checked[row.instanceId]) continue;
        const res = await commands.l10nApply(row.instanceId, lang);
        if (res.status !== 'ok') {
          results[row.instanceId] = formatError(res.error);
        } else {
          results[row.instanceId] = res.data ? 'applied' : 'deferred';
          // Both outcomes wrote the pack, so this row is done. Unticking is
          // what gives the dialog a terminal state: a second press then means
          // exactly one thing — retry what failed — and with nothing left
          // ticked the primary button disables itself.
          checked[row.instanceId] = false;
        }
        // Checked after the write, not before: the in-flight instance is
        // already half-done, so it finishes and gets its line.
        if (dismissed) break;
      }
    } finally {
      running = false;
      hasRun = true;
    }
  }
</script>

<Modal
  ariaLabelledby={titleId}
  onClose={closeSelf}
  closeOnBackdrop={!running}
  panelClass="w-[520px] max-w-full p-5 flex flex-col gap-3"
  dataTestid="apply-targets-dialog"
>
  <DialogTitle id={titleId}>{$t('instance.l10n.targets.title')}</DialogTitle>

  {#if !loaded}
    <LoadingPanel label={$t('common.loading')} />
  {:else}
    <ul class="flex flex-col gap-2">
      {#each rows as row (row.instanceId)}
        {@const reason = blockedReason(row)}
        {@const state = stateLabel(row.state)}
        <li
          class="flex flex-col gap-0.5 rounded border border-subtle p-2"
          data-testid={`apply-targets-row-${row.instanceId}`}
        >
          <label class="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={checked[row.instanceId] ?? false}
              disabled={!row.actionable}
              onchange={(e) => (checked[row.instanceId] = e.currentTarget.checked)}
              data-testid={`apply-targets-check-${row.instanceId}`}
            />
            <span class="text-primary">{row.name}</span>
            <!--
              The chip is the state as it was when the dialog loaded, and
              `rows` is never refetched. Once a row has a result that snapshot
              is stale, and showing both put "not applied" next to "Applied" on
              one row. Hidden rather than flipped: after a deferred apply "not
              applied" is still TRUE, so flipping it would invent a state the
              backend never returned.
            -->
            {#if state && !results[row.instanceId]}
              <span class="rounded bg-subtle px-1.5 py-0.5 text-xs text-secondary">{state}</span>
            {/if}
          </label>
          {#if reason}
            <span class="pl-6 text-xs text-muted">{reason}</span>
          {/if}
          <!--
            Exactly one Lucerna pack exists per instance, so applying replaces
            whatever language is currently on it. A bulk action must not
            silently overwrite a deliberate choice — say which language goes.
          -->
          {#if row.appliedOtherLang}
            <span class="pl-6 text-xs text-warning-text">
              {$t('instance.l10n.targets.replacesOther', { lang: row.appliedOtherLang })}
            </span>
          {/if}
          {#if results[row.instanceId]}
            <span
              class={`pl-6 text-xs ${resultClass(results[row.instanceId])}`}
              data-testid={`apply-targets-result-${row.instanceId}`}
            >
              {resultText(results[row.instanceId])}
            </span>
          {/if}
        </li>
      {/each}
    </ul>

    {#if rows.length === 0 || !rows.some((row) => row.actionable)}
      <p class="text-sm text-secondary" data-testid="apply-targets-none">
        {$t('instance.l10n.targets.noCandidates')}
      </p>
    {:else if tickedCount === 0}
      <p class="text-xs text-muted" data-testid="apply-targets-empty">
        {$t('instance.l10n.targets.nothingToShip')}
      </p>
    {/if}

    <div class="mt-2 flex justify-end gap-2">
      <button
        type="button"
        class="btn-ghost btn-sm"
        data-testid="apply-targets-cancel"
        onclick={closeSelf}
      >
        {hasRun ? $t('common.close') : $t('common.cancel')}
      </button>
      <BusyButton
        busy={running}
        disabled={tickedCount === 0}
        class="btn-primary btn-sm"
        data-testid="apply-targets-run"
        onclick={run}
      >
        {$t('instance.l10n.targets.applyAction')}
      </BusyButton>
    </div>
  {/if}
</Modal>
