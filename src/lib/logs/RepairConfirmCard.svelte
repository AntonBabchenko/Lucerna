<script lang="ts">
  import { t } from '$lib/i18n';
  import { displayLoader } from '$lib/instances/loader-display';
  import type { RepairPlan, RepairChoice, ConflictCandidate } from '$lib/ipc/bindings';
  import Spinner from '$lib/ui/Spinner.svelte';

  let {
    plan,
    onConfirm,
    onCancel,
    busy = false,
  }: {
    plan: RepairPlan;
    onConfirm: (choice: RepairChoice) => void;
    onCancel: () => void;
    busy?: boolean;
  } = $props();

  // For resolve_conflict: the selected candidate sha1 + mode.
  let selected = $state<{ sha1: string; mode: 'disable' | 'swap' } | null>(null);

  function chooseConflict(c: ConflictCandidate, mode: 'disable' | 'swap') {
    selected = { sha1: c.sha1, mode };
  }

  // For non-conflict plans, always confirmable. For conflict, the chosen
  // candidate must still exist and — for a swap — still have a swap target,
  // so a selection that goes stale (plan changes) re-disables Confirm rather
  // than silently no-op'ing on click.
  const canConfirm = $derived.by(() => {
    if (plan.kind !== 'resolve_conflict') return true;
    const sel = selected;
    if (!sel) return false;
    const cand = plan.candidates.find((c) => c.sha1 === sel.sha1);
    if (!cand) return false;
    return sel.mode === 'disable' || cand.swap_target !== null;
  });

  function confirm() {
    if (plan.kind === 'raise_heap') {
      onConfirm({ kind: 'raise_heap', to_mb: plan.to_mb });
    } else if (plan.kind === 'reinstall_loader') {
      onConfirm({ kind: 'reinstall_loader' });
    } else if (plan.kind === 'redownload_mod') {
      onConfirm({ kind: 'reinstall', old_sha1: plan.old_sha1, target: plan.target });
    } else if (plan.kind === 'resolve_conflict' && selected) {
      const cand = plan.candidates.find((c) => c.sha1 === selected!.sha1);
      if (!cand) return;
      if (selected.mode === 'disable') {
        onConfirm({ kind: 'disable_mod', sha1: cand.sha1 });
      } else if (cand.swap_target) {
        onConfirm({ kind: 'reinstall', old_sha1: cand.sha1, target: cand.swap_target });
      }
    }
  }
</script>

<div class="mt-3 rounded border border-warning-text/40 bg-surface p-3" data-testid="repair-card">
  {#if plan.kind === 'raise_heap'}
    <p class="text-sm">
      {$t('logs.repair.raiseHeap', { from: plan.from_mb, to: plan.to_mb })}
    </p>
  {:else if plan.kind === 'reinstall_loader'}
    <p class="text-sm">
      {$t('logs.repair.reinstallLoader', { loader: displayLoader(plan.loader) })}
    </p>
  {:else if plan.kind === 'redownload_mod'}
    <p class="text-sm">{$t('logs.repair.redownloadMod', { file: plan.filename })}</p>
  {:else if plan.kind === 'resolve_conflict'}
    <p id="repair-conflict-prompt" class="text-sm font-semibold">
      {$t('logs.repair.conflictPrompt')}
    </p>
    <!-- One shared radio group: a conflict is resolved by a single change
         (disable one mod, or update one to a compatible version), so exactly
         one option across all candidates is selectable. -->
    <div
      role="radiogroup"
      aria-labelledby="repair-conflict-prompt"
      class="mt-2 flex flex-col gap-1.5"
    >
      {#each plan.candidates as c (c.sha1)}
        <label class="flex items-center gap-2 text-sm">
          <input
            type="radio"
            name="conflict"
            data-testid={`conflict-disable-${c.sha1}`}
            checked={selected?.sha1 === c.sha1 && selected?.mode === 'disable'}
            disabled={busy}
            onchange={() => chooseConflict(c, 'disable')}
          />
          <span>{$t('logs.repair.disableNamed', { name: c.name })}</span>
          {#if c.compat_flagged}
            <span class="text-xs text-warning-text">{$t('logs.repair.compatHint')}</span>
          {/if}
        </label>
        {#if c.swap_target && c.swap_version_label}
          <label class="flex items-center gap-2 text-sm">
            <input
              type="radio"
              name="conflict"
              data-testid={`conflict-swap-${c.sha1}`}
              checked={selected?.sha1 === c.sha1 && selected?.mode === 'swap'}
              disabled={busy}
              onchange={() => chooseConflict(c, 'swap')}
            />
            <span
              >{$t('logs.repair.updateNamed', {
                name: c.name,
                version: c.swap_version_label,
              })}</span
            >
          </label>
        {/if}
      {/each}
    </div>
  {/if}

  <div class="mt-3 flex items-center gap-2">
    <button
      type="button"
      class="btn-primary btn-sm"
      data-testid="repair-confirm"
      disabled={!canConfirm || busy}
      onclick={confirm}
    >
      {#if busy}
        <span class="inline-flex items-center gap-1.5">
          <Spinner size="sm" label={$t('logs.repair.working')} />
          {$t('logs.repair.working')}
        </span>
      {:else}
        {$t('logs.repair.apply')}
      {/if}
    </button>
    <button
      type="button"
      class="btn-secondary btn-sm"
      data-testid="repair-cancel"
      disabled={busy}
      onclick={onCancel}
    >
      {$t('common.cancel')}
    </button>
  </div>
</div>
