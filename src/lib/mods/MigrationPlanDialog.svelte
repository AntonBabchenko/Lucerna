<script lang="ts">
  // Offers remediation for mods left behind by an MC-version/loader change —
  // never applies anything automatically (design decision #1). Fetches its
  // own plan on mount via `mods_plan_mc_migration`, lets the user settle a
  // per-row choice for every bucket, and posts the settled selection to
  // `mods_apply_mc_migration` only once Apply is explicitly clicked.
  //
  // `replaceable` and `new_dependencies` rows default UNCHECKED — "opt-in
  // like everything else" (see `docs/superpowers` spec). `stranded` rows
  // carry no default at all: the backend's `StrandedDisposition` has no
  // `Default` impl on purpose (a proven-incompatible jar has no safe
  // "the user didn't say" fallback), so Apply stays disabled until every
  // stranded row has an explicit disable/remove/keep choice — the backend
  // cannot enforce this itself without re-running the plan, so it is this
  // dialog's job (see `McMigrationSelections`'s doc comment in migration.rs).
  import { onMount } from 'svelte';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';
  import {
    commands,
    type McMigrationPlan_Serialize,
    type McMigrationReport,
    type McMigrationRowOutcome,
    type McMigrationSelections_Deserialize,
    type NewDependencyRow_Serialize,
    type StrandedDisposition,
    type StrandedReason,
    type StrandedRow,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import ToggleChipGroup from '$lib/ui/ToggleChipGroup.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import { tooltip } from '$lib/ui/tooltip';

  let {
    instanceId,
    onClose,
    onApplied,
  }: {
    instanceId: string;
    onClose: () => void;
    // Called once `mods_apply_mc_migration` returns successfully, before the
    // dialog switches to its result view — the caller's cue to refresh
    // whatever mod/compat state it owns.
    onApplied?: () => void;
  } = $props();

  type Phase = 'loading' | 'error' | 'plan' | 'result';

  let phase = $state<Phase>('loading');
  let plan = $state<McMigrationPlan_Serialize | null>(null);
  let loadError = $state<string | null>(null);

  // Per-row settled choices. Sets/maps from `svelte/reactivity` so `.add` /
  // `.delete` / `.set` mutate in place and stay reactive — no manual
  // reassignment needed (see e.g. `RunningInstancesPopover`'s `stopping`).
  const replaceChecked = new SvelteSet<string>();
  const depChecked = new SvelteSet<string>();
  const dispositions = new SvelteMap<string, StrandedDisposition>();

  let applying = $state(false);
  let applyError = $state<string | null>(null);
  let report = $state<McMigrationReport | null>(null);

  // `source:project_id` — a new-dependency row has no sha1 (nothing is
  // installed for it yet), so its identity for selection purposes is its
  // platform project, not a jar hash.
  function depKey(row: NewDependencyRow_Serialize): string {
    return `${row.source}:${row.project_id}`;
  }

  // A new-dependency is MANDATORY once the user checks a replace that needs it:
  // a dependency's `needed_by` carries the display names of the replaceable rows
  // whose target requires it. Reinstalling a mod WITHOUT the dependency the plan
  // surfaced for its target re-creates the exact pre-load crash this feature
  // exists to prevent (BiomesOPlenty mandatorily needs TerraBlender), while the
  // report would still read "Replaced". So such a dependency is force-included
  // and its checkbox locked for as long as the dependent replace stays checked;
  // it reverts to an optional, editable row the moment that replace is unchecked.
  const checkedReplaceNames = $derived(
    new Set((plan?.replaceable ?? []).filter((r) => replaceChecked.has(r.sha1)).map((r) => r.name)),
  );
  function depRequiredByReplace(row: NewDependencyRow_Serialize): boolean {
    return row.needed_by.some((n) => checkedReplaceNames.has(n));
  }
  function depIncluded(row: NewDependencyRow_Serialize): boolean {
    return depChecked.has(depKey(row)) || depRequiredByReplace(row);
  }

  async function loadPlan() {
    phase = 'loading';
    loadError = null;
    const res = await commands.modsPlanMcMigration(instanceId);
    if (res.status === 'error') {
      loadError = formatError(res.error);
      phase = 'error';
      return;
    }
    replaceChecked.clear();
    depChecked.clear();
    dispositions.clear();
    plan = res.data;
    phase = 'plan';
  }

  onMount(() => void loadPlan());

  // Apply is gated on "the user selected at least one thing to do", NOT on
  // "every stranded row is decided". Someone who only wants a top-section
  // reinstall must not be forced to rule on mods they are deferring: an
  // undecided stranded row is applied as `keep` (see `apply()`), a no-op that
  // leaves the jar in place — so nothing is decided for them and no data is
  // touched. Deferred mods stay flagged incompatible by the post-apply compat
  // re-scan (`onApplied`), so a partial apply never reads as "all fixed".
  const hasSelection = $derived(
    replaceChecked.size > 0 || depChecked.size > 0 || dispositions.size > 0,
  );
  const canApply = $derived(plan !== null && hasSelection);
  const nothingToDo = $derived(
    plan !== null &&
      plan.replaceable.length === 0 &&
      plan.new_dependencies.length === 0 &&
      plan.stranded.length === 0,
  );

  // Plain-language copy per `StrandedReason` — a failed query must never
  // read as "no build exists" (see the Rust doc comment on the enum).
  function reasonKey(reason: StrandedReason) {
    switch (reason.kind) {
      case 'no_build_for_target':
        return 'mods.migration.reason.noBuildForTarget' as const;
      case 'no_project_to_ask':
        return 'mods.migration.reason.noProjectToAsk' as const;
      case 'pack_origin':
        return 'mods.migration.reason.packOrigin' as const;
      case 'query_failed':
        return 'mods.migration.reason.queryFailed' as const;
      case 'loader_too_old':
        return 'mods.migration.reason.loaderTooOld' as const;
    }
  }

  function dispositionOptions(row: StrandedRow) {
    return [
      {
        value: 'disable',
        label: $t('mods.migration.disposition.disable'),
        tone: 'warning' as const,
        testId: `migration-disposition-disable-${row.sha1}`,
      },
      {
        value: 'remove',
        label: $t('mods.migration.disposition.remove'),
        tone: 'danger' as const,
        testId: `migration-disposition-remove-${row.sha1}`,
      },
      {
        value: 'keep',
        label: $t('mods.migration.disposition.keep'),
        tone: 'neutral' as const,
        testId: `migration-disposition-keep-${row.sha1}`,
      },
    ];
  }

  function outcomeBadgeLabel(kind: McMigrationRowOutcome['kind']): string {
    switch (kind) {
      case 'replaced':
        return $t('mods.migration.outcome.replaced');
      case 'installed_dependency':
        return $t('mods.migration.outcome.installedDependency');
      case 'disabled':
        return $t('mods.migration.outcome.disabled');
      case 'removed':
        return $t('mods.migration.outcome.removed');
      case 'failed':
        return $t('mods.migration.outcome.failedBadge');
    }
  }

  const succeededCount = $derived(
    report ? report.outcomes.filter((o) => o.kind !== 'failed').length : 0,
  );

  async function apply() {
    if (!plan || !canApply || applying) return;
    applying = true;
    applyError = null;
    // `target` values below are the EXACT ones the plan already resolved —
    // never re-derived here. See `McMigrationSelections`'s doc comment for
    // why (a re-fetch could install a build different from the one reviewed,
    // and would reopen the dependency-expansion bug this feature fixes).
    const selections: McMigrationSelections_Deserialize = {
      replace: plan.replaceable
        .filter((row) => replaceChecked.has(row.sha1))
        .map((row) => ({ old_sha1: row.sha1, target: row.target })),
      new_dependencies: plan.new_dependencies
        .filter((row) => depIncluded(row))
        .map((row) => row.target),
      stranded: plan.stranded.map((row) => ({
        sha1: row.sha1,
        // Undecided rows are kept — a no-op that leaves the jar in place. The
        // user deferred them; never drop a stranded row from the payload, and
        // never default it to a destructive disable/remove.
        disposition: dispositions.get(row.sha1) ?? 'keep',
      })),
    };
    const res = await commands.modsApplyMcMigration(instanceId, selections);
    applying = false;
    if (res.status === 'error') {
      applyError = formatError(res.error);
      return;
    }
    report = res.data;
    phase = 'result';
    onApplied?.();
  }
</script>

<Modal
  ariaLabelledby="migration-plan-title"
  {onClose}
  panelClass="w-full max-w-2xl max-h-[85vh] flex flex-col"
  dataTestid="migration-plan-dialog"
  closeOnBackdrop={!applying}
  closeOnEscape={!applying}
>
  <header class="shrink-0 flex items-center justify-between border-b p-4">
    <h2 id="migration-plan-title" class="text-base font-semibold text-primary">
      {$t('mods.migration.title')}
    </h2>
    <CloseButton onClick={onClose} />
  </header>

  <div class="min-h-0 flex-1 overflow-y-auto p-4">
    {#if phase === 'loading'}
      <LoadingPanel label={$t('mods.migration.loading')} />
    {:else if phase === 'error'}
      <p class="text-sm text-danger mb-3" data-testid="migration-load-error">{loadError}</p>
      <button type="button" class="btn-secondary btn-sm" onclick={() => void loadPlan()}>
        {$t('mods.migration.retryBtn')}
      </button>
    {:else if phase === 'result' && report}
      <p class="text-sm text-secondary mb-3" data-testid="migration-result-summary">
        {$t('mods.migration.resultSummary', {
          succeeded: succeededCount,
          total: report.outcomes.length,
        })}
      </p>
      <ul class="space-y-1">
        {#each report.outcomes as outcome, i (i)}
          <li
            class="flex flex-col gap-0.5 rounded border border-border-subtle p-2"
            data-testid={`migration-result-row-${i}`}
          >
            <div class="flex items-center gap-2">
              <StatusBadge variant={outcome.kind === 'failed' ? 'danger' : 'success'}>
                {outcomeBadgeLabel(outcome.kind)}
              </StatusBadge>
              <span class="text-sm text-primary truncate">{outcome.name}</span>
            </div>
            {#if outcome.kind === 'failed'}
              <p class="text-xs text-danger">{formatError(outcome.error)}</p>
            {/if}
          </li>
        {/each}
      </ul>
    {:else if plan}
      <p class="text-sm text-secondary mb-1" data-testid="migration-summary">
        {$t('mods.migration.summary', {
          fits: plan.fits.length,
          replaceable: plan.replaceable.length,
          stranded: plan.stranded.length,
        })}
      </p>
      {#if plan.unjudged > 0}
        <p class="text-xs text-muted mb-3" data-testid="migration-unjudged">
          {$t('mods.migration.unjudgedLine', { count: plan.unjudged })}
        </p>
      {/if}

      {#if nothingToDo}
        <p class="text-sm text-secondary" data-testid="migration-nothing-to-do">
          {$t('mods.migration.nothingToDo')}
        </p>
      {:else}
        {#if plan.replaceable.length > 0}
          <section class="mb-4" data-testid="migration-replaceable-section">
            <h3 class="mb-2 text-xs uppercase tracking-wide text-muted">
              {$t('mods.migration.replaceableHeading', { count: plan.replaceable.length })}
            </h3>
            <ul class="space-y-1">
              {#each plan.replaceable as row (row.sha1)}
                <li
                  class="flex items-start gap-2 rounded border border-border-subtle p-2"
                  data-testid={`migration-replaceable-row-${row.sha1}`}
                >
                  <input
                    type="checkbox"
                    class="mt-0.5"
                    checked={replaceChecked.has(row.sha1)}
                    onchange={(e) => {
                      if ((e.currentTarget as HTMLInputElement).checked) {
                        replaceChecked.add(row.sha1);
                      } else {
                        replaceChecked.delete(row.sha1);
                      }
                    }}
                    aria-label={row.name}
                  />
                  <div class="min-w-0 flex-1">
                    <div class="text-sm text-primary truncate">{row.name}</div>
                    <div class="text-xs text-muted truncate">
                      {$t('mods.migration.replaceableTarget', {
                        version: row.target.version_number,
                      })}
                    </div>
                  </div>
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        {#if plan.new_dependencies.length > 0}
          <section class="mb-4" data-testid="migration-new-deps-section">
            <h3 class="mb-2 text-xs uppercase tracking-wide text-muted">
              {$t('mods.migration.newDependenciesHeading', {
                count: plan.new_dependencies.length,
              })}
            </h3>
            <ul class="space-y-1">
              {#each plan.new_dependencies as row (depKey(row))}
                <li
                  class="flex items-start gap-2 rounded border border-border-subtle p-2"
                  data-testid={`migration-new-dep-row-${depKey(row)}`}
                >
                  <input
                    type="checkbox"
                    class="mt-0.5"
                    checked={depIncluded(row)}
                    disabled={depRequiredByReplace(row)}
                    onchange={(e) => {
                      if ((e.currentTarget as HTMLInputElement).checked) {
                        depChecked.add(depKey(row));
                      } else {
                        depChecked.delete(depKey(row));
                      }
                    }}
                    aria-label={row.target.name}
                  />
                  <div class="min-w-0 flex-1">
                    <div class="text-sm text-primary truncate">{row.target.name}</div>
                    <div class="text-xs text-muted truncate">
                      {$t('mods.migration.neededBy', { names: row.needed_by.join(', ') })}
                    </div>
                  </div>
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        {#if plan.stranded.length > 0}
          <section data-testid="migration-stranded-section">
            <h3 class="mb-2 text-xs uppercase tracking-wide text-muted">
              {$t('mods.migration.strandedHeading', { count: plan.stranded.length })}
            </h3>
            <ul class="space-y-2">
              {#each plan.stranded as row (row.sha1)}
                <li
                  class="rounded border border-border-subtle p-2"
                  data-testid={`migration-stranded-row-${row.sha1}`}
                >
                  <div class="text-sm text-primary">{row.name}</div>
                  <p class="mb-1.5 text-xs text-muted">{$t(reasonKey(row.reason))}</p>
                  <ToggleChipGroup
                    options={dispositionOptions(row)}
                    value={dispositions.get(row.sha1) ?? ''}
                    ariaLabel={$t('mods.migration.dispositionAriaLabel', { name: row.name })}
                    onChange={(v) => dispositions.set(row.sha1, v as StrandedDisposition)}
                  />
                </li>
              {/each}
            </ul>
          </section>
        {/if}
      {/if}
    {/if}
  </div>

  {#if phase === 'plan' && applyError}
    <div class="shrink-0 border-t px-4 py-2">
      <p class="text-sm text-danger" data-testid="migration-apply-error">{applyError}</p>
    </div>
  {/if}

  <footer class="shrink-0 flex items-center justify-between gap-2 border-t p-3">
    {#if phase === 'plan' && plan && !nothingToDo}
      <p class="text-xs text-muted" data-testid="migration-apply-disabled-reason">
        {!canApply ? $t('mods.migration.applyDisabledReason') : ''}
      </p>
    {:else}
      <span></span>
    {/if}
    <div class="flex gap-2">
      {#if phase === 'result'}
        <button
          type="button"
          class="btn-primary btn-sm"
          data-testid="migration-done-btn"
          onclick={onClose}
        >
          {$t('mods.migration.doneBtn')}
        </button>
      {:else if phase === 'plan' && plan}
        <button type="button" class="btn-secondary btn-sm" onclick={onClose}>
          {nothingToDo ? $t('common.close') : $t('common.cancel')}
        </button>
        {#if !nothingToDo}
          <span
            use:tooltip={{
              text: !canApply ? $t('mods.migration.applyDisabledReason') : '',
              describe: false,
            }}
          >
            <BusyButton
              busy={applying}
              disabled={!canApply}
              class="btn-primary btn-sm"
              data-testid="migration-apply-btn"
              onclick={() => void apply()}
            >
              {$t('mods.migration.applyBtn')}
            </BusyButton>
          </span>
        {/if}
      {:else}
        <button type="button" class="btn-secondary btn-sm" onclick={onClose}>
          {$t('common.close')}
        </button>
      {/if}
    </div>
  </footer>
</Modal>
