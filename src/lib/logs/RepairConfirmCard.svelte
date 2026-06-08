<script lang="ts">
  import { t } from '$lib/i18n';
  import type { RepairPlan, RepairChoice, ConflictCandidate } from '$lib/ipc/bindings';

  let {
    plan,
    onConfirm,
    onCancel,
  }: {
    plan: RepairPlan;
    onConfirm: (choice: RepairChoice) => void;
    onCancel: () => void;
  } = $props();

  // For resolve_conflict: the selected candidate sha1 + mode.
  let selected = $state<{ sha1: string; mode: 'disable' | 'swap' } | null>(null);

  function chooseConflict(c: ConflictCandidate, mode: 'disable' | 'swap') {
    selected = { sha1: c.sha1, mode };
  }

  const canConfirm = $derived(plan.kind !== 'resolve_conflict' || selected !== null);

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
    <p class="text-sm">{$t('logs.repair.reinstallLoader', { loader: plan.loader })}</p>
  {:else if plan.kind === 'redownload_mod'}
    <p class="text-sm">{$t('logs.repair.redownloadMod', { file: plan.filename })}</p>
  {:else if plan.kind === 'resolve_conflict'}
    <p class="text-sm font-semibold">{$t('logs.repair.conflictPrompt')}</p>
    <div class="mt-2 flex flex-col gap-2">
      {#each plan.candidates as c (c.sha1)}
        <div class="rounded border border-border-subtle p-2">
          <div class="flex items-center gap-2 text-sm">
            <span class="font-medium">{c.name}</span>
            {#if c.compat_flagged}
              <span class="text-xs text-warning-text">{$t('logs.repair.compatHint')}</span>
            {/if}
          </div>
          <div class="mt-1 flex gap-3 text-sm">
            <label class="flex items-center gap-1">
              <input
                type="radio"
                name="conflict"
                data-testid={`conflict-disable-${c.sha1}`}
                checked={selected?.sha1 === c.sha1 && selected?.mode === 'disable'}
                onchange={() => chooseConflict(c, 'disable')}
              />
              {$t('logs.repair.disableThis')}
            </label>
            {#if c.swap_target && c.swap_version_label}
              <label class="flex items-center gap-1">
                <input
                  type="radio"
                  name="conflict"
                  data-testid={`conflict-swap-${c.sha1}`}
                  checked={selected?.sha1 === c.sha1 && selected?.mode === 'swap'}
                  onchange={() => chooseConflict(c, 'swap')}
                />
                {$t('logs.repair.swapTo', { version: c.swap_version_label })}
              </label>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}

  <div class="mt-3 flex gap-2">
    <button
      type="button"
      class="btn-primary btn-sm"
      data-testid="repair-confirm"
      disabled={!canConfirm}
      onclick={confirm}
    >
      {$t('logs.repair.apply')}
    </button>
    <button
      type="button"
      class="btn-secondary btn-sm"
      data-testid="repair-cancel"
      onclick={onCancel}
    >
      {$t('common.cancel')}
    </button>
  </div>
</div>
