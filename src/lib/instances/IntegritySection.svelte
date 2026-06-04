<script lang="ts">
  import { t } from 'svelte-i18n';
  import { enqueueIntegrity, integrityStatusFor } from '$lib/instances/integrity-ops.svelte';
  import type { IntegrityStatus, VerifyCategory } from '$lib/ipc/bindings';

  // Observer view: the verify/repair op is owned by the page-level
  // integrity-ops store, not this section. The section reads the live op phase
  // (running/queued) for this instance, and otherwise renders the persisted
  // `status` passed reactively from `selected.integrity` — which the page
  // refreshes when an op completes (completionTick effect). No local state
  // machine, no remount-reset, no stale-response guard.

  let {
    instanceId,
    isRunning = false,
    name,
    status = null,
  }: {
    instanceId: string;
    isRunning?: boolean;
    name: string;
    status?: IntegrityStatus | null;
  } = $props();

  const op = $derived(integrityStatusFor(instanceId));
  // Buttons are blocked while the game runs OR while an op for this instance is
  // running/queued (dedupe is also enforced in the store, but disabling gives
  // the user feedback).
  const blocked = $derived(isRunning || op !== null);

  const catKey: Record<VerifyCategory, string> = {
    client: 'instance.integrity.catClient',
    libraries: 'instance.integrity.catLibraries',
    assets: 'instance.integrity.catAssets',
    jre: 'instance.integrity.catJre',
    profile_json: 'instance.integrity.catProfileJson',
  };

  function percent(done: number, total: number): number {
    if (total <= 0) return 0;
    return Math.min(100, (done / total) * 100);
  }
</script>

<section class="pt-3 border-t" data-tour-ctx="manage-integrity">
  <div class="flex items-start justify-between gap-3">
    <div>
      <h3 class="text-xs uppercase text-secondary mb-1">{$t('instance.integrity.heading')}</h3>
      <p class="text-xs text-muted max-w-xs">{$t('instance.integrity.subtitle')}</p>
    </div>
    <button
      type="button"
      class="btn-secondary btn-sm"
      disabled={blocked}
      title={isRunning ? $t('instance.integrity.busy') : ''}
      onclick={() => enqueueIntegrity(instanceId, name, 'verify')}
    >
      {status ? $t('instance.integrity.reverifyBtn') : $t('instance.integrity.verifyBtn')}
    </button>
  </div>

  {#if op?.phase === 'running'}
    <div class="mt-2" aria-live="polite">
      <p class="text-xs text-muted">
        {$t('instance.integrity.verifying', {
          values: { done: op.filesDone, total: op.filesTotal },
        })}
      </p>
      <div class="h-2 bg-subtle rounded overflow-hidden mt-1">
        <div
          class="h-full bg-accent transition-all"
          style="width: {percent(op.filesDone, op.filesTotal)}%"
        ></div>
      </div>
    </div>
  {:else if op?.phase === 'queued'}
    <p class="mt-2 text-xs text-muted" aria-live="polite">
      {$t('instance.integrity.statusQueued')}
    </p>
  {:else if status}
    {#if status.healthy}
      <p class="mt-2 text-sm text-success">✓ {$t('instance.integrity.allOk')}</p>
    {:else}
      <ul class="mt-2 space-y-1" aria-live="polite">
        {#each status.categories as cat (cat.category)}
          {@const bad = cat.missing + cat.corrupt}
          <li class="flex items-center justify-between text-xs">
            <span class="flex items-center gap-1.5">
              <span aria-hidden="true">{bad === 0 ? '✓' : '⚠'}</span>
              {$t(catKey[cat.category])}
            </span>
            <span class={bad === 0 ? 'text-muted' : 'text-danger'}>
              {bad === 0
                ? $t('instance.integrity.countOk', {
                    values: { ok: cat.ok, total: cat.total },
                  })
                : $t('instance.integrity.countProblems', {
                    values: { corrupt: cat.corrupt, missing: cat.missing },
                  })}
            </span>
          </li>
        {/each}
      </ul>
      <div class="mt-2 flex justify-end">
        <button
          type="button"
          class="btn-primary btn-sm"
          disabled={blocked}
          title={isRunning ? $t('instance.integrity.busy') : ''}
          onclick={() => enqueueIntegrity(instanceId, name, 'repair')}
        >
          {$t('instance.integrity.repairBtn', { values: { count: status.problem_count } })}
        </button>
      </div>
    {/if}
    {#if status.checked_unix_ms !== null}
      <p class="text-xs text-muted mt-1">
        {$t('instance.integrity.checkedAt', {
          values: { date: new Date(status.checked_unix_ms).toLocaleString() },
        })}
      </p>
    {/if}
  {:else}
    <p class="mt-2 text-xs text-muted">{$t('instance.integrity.statusNotChecked')}</p>
  {/if}
</section>
