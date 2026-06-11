<script lang="ts">
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { Icon } from '$lib/ui/icons';
  import { effectiveIntegrityStatus } from '$lib/instances/integrity-freshness';
  import { enqueueIntegrity, opStatusFor } from '$lib/ops/op-queue.svelte';
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

  const op = $derived(opStatusFor(instanceId));
  // A persisted "healthy" result from a previous launcher session is treated as
  // not-checked (session-scoped confidence); problem results persist. See
  // integrity-freshness.ts. The live op branches below take precedence over this.
  const effective = $derived(effectiveIntegrityStatus(status));
  // Buttons are blocked while the game runs OR while an op for this instance is
  // running/queued (dedupe is also enforced in the store, but disabling gives
  // the user feedback).
  const blocked = $derived(isRunning || op !== null);
  // Non-empty tooltip whenever a button is disabled, so a blocked click isn't
  // silent: game-running vs an integrity op already pending for this instance.
  const blockTitle = $derived(
    isRunning ? $t('instance.integrity.busy') : op ? $t('instance.integrity.statusQueued') : '',
  );

  const catKey: Record<VerifyCategory, TranslationKey> = {
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
      title={blockTitle}
      onclick={() => enqueueIntegrity(instanceId, name, 'verify')}
    >
      {effective ? $t('instance.integrity.reverifyBtn') : $t('instance.integrity.verifyBtn')}
    </button>
  </div>

  {#if op?.phase === 'running'}
    <div class="mt-2" aria-live="polite">
      <p class="text-xs text-muted">
        {op.kind === 'repair'
          ? $t('instance.integrity.repairing', { done: op.filesDone, total: op.filesTotal })
          : $t('instance.integrity.verifying', { done: op.filesDone, total: op.filesTotal })}
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
  {:else if effective}
    {#if effective.healthy}
      <p class="mt-2 text-sm text-success flex items-center gap-1.5">
        <Icon name="success" />
        {$t('instance.integrity.allOk')}
      </p>
    {:else}
      <ul class="mt-2 space-y-1" aria-live="polite">
        {#each effective.categories as cat (cat.category)}
          {@const bad = cat.missing + cat.corrupt}
          <li class="flex items-center justify-between text-xs">
            <span class="flex items-center gap-1.5">
              <Icon name={bad === 0 ? 'success' : 'warning'} />
              {$t(catKey[cat.category])}
            </span>
            <span class={bad === 0 ? 'text-muted' : 'text-danger'}>
              {bad === 0
                ? $t('instance.integrity.countOk', { ok: cat.ok, total: cat.total })
                : $t('instance.integrity.countProblems', {
                    corrupt: cat.corrupt,
                    missing: cat.missing,
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
          title={blockTitle}
          onclick={() => enqueueIntegrity(instanceId, name, 'repair')}
        >
          {$t('instance.integrity.repairBtn', { count: effective.problem_count })}
        </button>
      </div>
    {/if}
    {#if effective.checked_unix_ms !== null}
      <p class="text-xs text-muted mt-1">
        {$t('instance.integrity.checkedAt', {
          date: new Date(effective.checked_unix_ms).toLocaleString(),
        })}
      </p>
    {/if}
  {:else}
    <p class="mt-2 text-xs text-muted">{$t('instance.integrity.statusNotChecked')}</p>
  {/if}
</section>
