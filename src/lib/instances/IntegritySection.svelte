<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { t } from 'svelte-i18n';
  import type { VerifyCategory } from '$lib/ipc/bindings';
  import { createIntegrity } from '$lib/instances/integrity.svelte';

  let {
    instanceId,
    isRunning = false,
    initial = null,
    onChanged = () => {},
  }: {
    instanceId: string;
    isRunning?: boolean;
    initial?: import('$lib/ipc/bindings').IntegrityStatus | null;
    onChanged?: () => void;
  } = $props();

  // `initial` is a one-shot snapshot of the instance's persisted status;
  // {#key instanceId} remounts this component on switch, so re-capturing the
  // current value here is intentional (untrack documents that).
  const integ = createIntegrity(
    () => instanceId,
    () => isRunning,
    untrack(() => initial),
    // Capture the callback by reference so a verify that finishes AFTER the
    // user switched instances (this component unmounted) can still refresh
    // the list — reading the prop post-unmount would be undefined.
    untrack(() => onChanged),
  );
  onDestroy(() => integ.dispose());

  const catKey: Record<VerifyCategory, string> = {
    client: 'instance.integrity.catClient',
    libraries: 'instance.integrity.catLibraries',
    assets: 'instance.integrity.catAssets',
    jre: 'instance.integrity.catJre',
    profile_json: 'instance.integrity.catProfileJson',
  };
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
      disabled={isRunning || integ.state === 'verifying' || integ.state === 'repairing'}
      title={isRunning ? $t('instance.integrity.busy') : ''}
      onclick={() => integ.verify()}
    >
      {integ.hasResult ? $t('instance.integrity.reverifyBtn') : $t('instance.integrity.verifyBtn')}
    </button>
  </div>

  {#if integ.state === 'verifying' || integ.state === 'repairing'}
    <div class="mt-2" aria-live="polite">
      <p class="text-xs text-muted">
        {integ.state === 'verifying'
          ? $t('instance.integrity.verifying', {
              values: { done: integ.filesDone, total: integ.filesTotal },
            })
          : $t('instance.integrity.repairing', {
              values: { done: integ.filesDone, total: integ.filesTotal },
            })}
      </p>
      <div class="h-2 bg-subtle rounded overflow-hidden mt-1">
        <div
          class="h-full bg-accent transition-all"
          style="width: {integ.filesTotal > 0 ? (integ.filesDone / integ.filesTotal) * 100 : 0}%"
        ></div>
      </div>
    </div>
  {/if}

  {#if integ.state === 'report' && integ.hasResult}
    {#if integ.healthy === true}
      <p class="mt-2 text-sm text-success">✓ {$t('instance.integrity.allOk')}</p>
    {:else}
      <ul class="mt-2 space-y-1" aria-live="polite">
        {#each integ.categories as cat (cat.category)}
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
          disabled={isRunning}
          title={isRunning ? $t('instance.integrity.busy') : ''}
          onclick={() => integ.repair()}
        >
          {$t('instance.integrity.repairBtn', { values: { count: integ.problemCount } })}
        </button>
      </div>
    {/if}
    {#if integ.checkedAt}
      <p class="text-xs text-muted mt-1">
        {$t('instance.integrity.checkedAt', {
          values: { date: new Date(integ.checkedAt).toLocaleString() },
        })}
      </p>
    {/if}
  {/if}

  {#if integ.error}
    <p class="mt-2 text-xs text-danger">{integ.error}</p>
  {/if}
</section>
