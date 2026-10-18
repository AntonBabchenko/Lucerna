<script lang="ts">
  import { locale, t } from '$lib/i18n';
  import { formatDateTime } from '$lib/format/date-time';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { Icon } from '$lib/ui/icons';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import { effectiveIntegrityStatus } from '$lib/instances/integrity-freshness';
  import { enqueueIntegrity } from '$lib/ops/op-queue.svelte';
  import { taskFor } from '$lib/tasks/registry.svelte';
  import {
    commands,
    type IntegrityStatus,
    type JavaChoice,
    type VerifyCategory,
  } from '$lib/ipc/bindings';
  import { settingsRowFor } from '$lib/java/default-option';

  // Observer view: the verify/repair op is owned by the task registry
  // (`$lib/tasks/registry.svelte`), not this section — `op-queue.svelte.ts`
  // only wraps the enqueue call + its toast/report side effects now (see that
  // module's doc comment). The section reads the live task (running/queued)
  // for this instance via `taskFor`, and otherwise renders the persisted
  // `status` passed reactively from `selected.integrity` — which the page
  // refreshes when an op completes (completionTick effect). No local state
  // machine, no remount-reset, no stale-response guard.
  //
  // `taskFor` matches by SCOPE alone, not kind, so a running clone of this
  // same instance also blocks Verify/Repair here — a deliberate widening from
  // the old `op-queue.svelte.ts`'s integrity-only dedupe (see that module's
  // doc comment for why the old kind-scoped guard isn't reproduced).

  let {
    instanceId,
    isRunning = false,
    name,
    status = null,
    java = null,
    requirementKey = '',
  }: {
    instanceId: string;
    isRunning?: boolean;
    name: string;
    status?: IntegrityStatus | null;
    /** The instance's own Java choice; `null` follows Settings. */
    java?: JavaChoice | null;
    /** Changes when the version, and so the Java major it needs, may have. */
    requirementKey?: string;
  } = $props();

  // A Java the user chose is not Lucerna's to check or repair, so the check has
  // no row for it (spec 2026-10-11-custom-java §7): launch probes it every time
  // instead, and this line says so. «Default» counts when Settings names a Java
  // for the major this version needs; a need that could not be read shows no
  // line rather than a guess.
  let required = $state<number | null>(null);
  $effect(() => {
    void requirementKey;
    let stale = false;
    void loadRequired(instanceId, () => stale);
    return () => {
      stale = true;
    };
  });
  async function loadRequired(id: string, isStale: () => boolean) {
    let major: number | null = null;
    try {
      const r = await commands.instanceJavaRequirement(id);
      if (r.status === 'ok') major = r.data;
    } catch {
      // Unknown: no line, as for a typed error.
    }
    if (!isStale()) required = major;
  }
  const ownJava = $derived(
    java?.kind === 'custom' || (java === null && settingsRowFor(required).kind === 'row'),
  );

  const op = $derived(taskFor({ instanceId }));
  // A persisted "healthy" result from a previous launcher session is treated as
  // not-checked (session-scoped confidence); problem results persist. See
  // integrity-freshness.ts. The live op branches below take precedence over this.
  const effective = $derived(effectiveIntegrityStatus(status));
  // A report saved before the count existed has none (`?? 0`).
  const anyBuiltDifferently = $derived(
    effective?.categories.some((c) => (c.built_differently ?? 0) > 0) ?? false,
  );
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

<section data-tour-ctx="settings-integrity">
  <div class="flex items-start justify-between gap-3">
    <div>
      <h3 class="mb-1 text-sm font-medium text-primary">{$t('instance.integrity.heading')}</h3>
      <p class="text-xs text-muted max-w-xs">{$t('instance.integrity.subtitle')}</p>
    </div>
    <span class="inline-flex" use:tooltip={{ text: blockTitle, describe: false }}>
      <button
        type="button"
        class="btn-secondary btn-sm"
        disabled={blocked}
        onclick={() => enqueueIntegrity(instanceId, name, 'verify')}
      >
        {effective ? $t('instance.integrity.reverifyBtn') : $t('instance.integrity.verifyBtn')}
      </button>
    </span>
  </div>

  {#if op?.kind === 'verify' || op?.kind === 'repair'}
    {@const done = op.progress?.current ?? 0}
    {@const total = op.progress?.total ?? 0}
    <div class="mt-2" aria-live="polite">
      <div class="flex items-center gap-2">
        <Spinner size="sm" />
        <p class="text-xs text-muted">
          {op.kind === 'repair'
            ? $t('instance.integrity.repairing', { done, total })
            : $t('instance.integrity.verifying', { done, total })}
        </p>
      </div>
      <div class="h-2 bg-subtle rounded overflow-hidden mt-1">
        <div class="h-full bg-accent transition-all" style="width: {percent(done, total)}%"></div>
      </div>
    </div>
  {:else if op !== null}
    <!-- Blocked by an active task of a DIFFERENT kind for this same instance
         (e.g. a clone in flight) — `taskFor` matches by scope alone, see the
         module doc comment above. No per-kind progress to show, so this falls
         back to the same "pending" wording the old queued-integrity branch
         used. -->
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
          {@const rebuilt = cat.built_differently ?? 0}
          <li class="flex items-center justify-between text-xs">
            <span class="flex items-center gap-1.5">
              <Icon name={bad > 0 ? 'warning' : rebuilt > 0 ? 'info' : 'success'} />
              {$t(catKey[cat.category])}
            </span>
            <span class="flex flex-col items-end">
              {#if bad > 0}
                <span class="text-danger">
                  {$t('instance.integrity.countProblems', {
                    corrupt: cat.corrupt,
                    missing: cat.missing,
                  })}
                </span>
              {/if}
              {#if rebuilt > 0}
                <span class="text-warning-text">
                  {$t('instance.integrity.countBuiltDifferently', { count: rebuilt })}
                </span>
              {/if}
              {#if bad === 0 && rebuilt === 0}
                <span class="text-muted">
                  {$t('instance.integrity.countOk', { ok: cat.ok, total: cat.total })}
                </span>
              {/if}
            </span>
          </li>
        {/each}
      </ul>
      {#if anyBuiltDifferently}
        <!-- Not damage (spec 2026-10-16 §5.5): Forge files an older Lucerna
             built with its own code. Say so, and what Repair does. -->
        <p
          class="mt-2 text-xs text-muted flex items-start gap-1.5"
          data-testid="integrity-built-differently"
        >
          <Icon name="info" />
          {$t('instance.integrity.builtDifferentlyNote')}
        </p>
      {/if}
      <div class="mt-2 flex justify-end">
        <span class="inline-flex" use:tooltip={{ text: blockTitle, describe: false }}>
          <button
            type="button"
            class="btn-primary btn-sm"
            disabled={blocked}
            onclick={() => enqueueIntegrity(instanceId, name, 'repair')}
          >
            {$t('instance.integrity.repairBtn', { count: effective.problem_count })}
          </button>
        </span>
      </div>
    {/if}
    {#if effective.checked_unix_ms !== null}
      <p class="text-xs text-muted mt-1">
        {$t('instance.integrity.checkedAt', {
          date: formatDateTime($locale, effective.checked_unix_ms),
        })}
      </p>
    {/if}
  {:else}
    <p class="mt-2 text-xs text-muted">{$t('instance.integrity.statusNotChecked')}</p>
  {/if}
  {#if ownJava}
    <p class="mt-2 text-xs text-muted flex items-center gap-1.5" data-testid="integrity-own-java">
      <Icon name="info" />
      {$t('instance.integrity.ownJava')}
    </p>
  {/if}
</section>
