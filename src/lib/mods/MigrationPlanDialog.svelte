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
  // `no_platform_build` rows are NOT proven incompatible — their section says
  // so — but they settle the same way: no default, undecided = keep.
  import { onDestroy, onMount } from 'svelte';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';
  import {
    commands,
    type InstanceWithStatus,
    type McMigrationPlan_Serialize,
    type McMigrationReport,
    type McMigrationRowOutcome,
    type McMigrationSelections_Deserialize,
    type MigrationPlanProgress,
    type NewDependencyRow_Serialize,
    type NoPlatformBuildRow,
    type StrandedDisposition,
    type StrandedReason,
    type StrandedRow,
    type UnjudgedReason,
  } from '$lib/ipc/bindings';
  import { loadMigrationPlan } from '$lib/mods/migration-plan-runner';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import PackPlatformRestoreDialog from '$lib/modpacks/PackPlatformRestoreDialog.svelte';
  import {
    type ConfirmOptions,
    createModpackUpdateFlow,
  } from '$lib/modpacks/modpack-update-flow.svelte';
  import { displayLoaderTag } from '$lib/instances/loader-display';
  import { instancesChanged, modpacksNav } from '$lib/settings/state.svelte';
  import { taskFor } from '$lib/tasks/registry.svelte';
  import Banner from '$lib/ui/Banner.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Icon from '$lib/ui/icons/Icon.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import SelectAllCheckbox from '$lib/ui/SelectAllCheckbox.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import ToggleChipGroup from '$lib/ui/ToggleChipGroup.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import { tooltip } from '$lib/ui/tooltip';

  let {
    instanceId,
    onClose,
    onApplied,
    onPlanLoaded,
  }: {
    instanceId: string;
    onClose: () => void;
    // Called once `mods_apply_mc_migration` returns successfully, before the
    // dialog switches to its result view — the caller's cue to refresh
    // whatever mod/compat state it owns.
    onApplied?: () => void;
    // Called once a plan has loaded successfully. The opener uses it to re-run
    // the chip's live check: the chip's verdicts never expire within a platform
    // triple while this plan is fetched fresh on every open, so without it the
    // two could disagree on screen. Fired AFTER the load so the chip's check is
    // served from the version cache this plan's probes just warmed.
    onPlanLoaded?: () => void;
  } = $props();

  type Phase = 'loading' | 'error' | 'plan' | 'result';

  let phase = $state<Phase>('loading');
  let plan = $state<McMigrationPlan_Serialize | null>(null);
  let loadError = $state<string | null>(null);
  // The backend's latest progress tick while the plan loads; `null` before the
  // first one, when the loading state reads exactly as it always has.
  let progress = $state<MigrationPlanProgress | null>(null);

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

  // Master checkbox over the replaceable section (maintainer request during the
  // Forge→Fabric smoke: ten rows, no way to take them all at once). Checking it
  // checks every replace row — the mandatory new-dependency rows then follow
  // through the existing needed_by force-include; unchecking releases them.
  const allReplaceChecked = $derived(
    (plan?.replaceable.length ?? 0) > 0 &&
      (plan?.replaceable ?? []).every((r) => replaceChecked.has(r.sha1)),
  );
  const someReplaceChecked = $derived(
    (plan?.replaceable ?? []).some((r) => replaceChecked.has(r.sha1)),
  );
  function toggleAllReplace(checkAll: boolean) {
    if (!plan) return;
    if (checkAll) {
      for (const r of plan.replaceable) replaceChecked.add(r.sha1);
    } else {
      for (const r of plan.replaceable) replaceChecked.delete(r.sha1);
    }
  }
  function depRequiredByReplace(row: NewDependencyRow_Serialize): boolean {
    return row.needed_by.some((n) => checkedReplaceNames.has(n));
  }
  function depIncluded(row: NewDependencyRow_Serialize): boolean {
    return depChecked.has(depKey(row)) || depRequiredByReplace(row);
  }

  async function loadPlan() {
    phase = 'loading';
    loadError = null;
    progress = null;
    const res = await loadMigrationPlan(instanceId, (p) => (progress = p));
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
    onPlanLoaded?.();
  }

  onMount(() => void loadPlan());

  // The loading label names what the backend is doing now; before its first
  // tick — and while it checks installed mods — it reads as it always has.
  function loadingLabel(p: MigrationPlanProgress | null): string {
    switch (p?.phase) {
      case 'finding_replacements':
        return $t('mods.migration.progress.findingReplacements');
      case 'resolving_dependencies':
        return $t('mods.migration.progress.resolvingDependencies');
      default:
        return $t('mods.migration.loading');
    }
  }

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
      plan.stranded.length === 0 &&
      plan.no_platform_build.length === 0,
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

  // One sentence per REAL state — a missing CurseForge key or an offline
  // machine must never be worded as a claim about the mod.
  function unjudgedReasonKey(reason: UnjudgedReason) {
    switch (reason) {
      case 'unreadable':
        return 'mods.migration.unjudgedReason.unreadable' as const;
      case 'pack_owned':
        return 'mods.migration.unjudgedReason.packOwned' as const;
      case 'no_mod_page':
        return 'mods.migration.unjudgedReason.noModPage' as const;
      case 'no_loader':
        return 'mods.migration.unjudgedReason.noLoader' as const;
      case 'platform_unavailable':
        return 'mods.migration.unjudgedReason.platformUnavailable' as const;
      case 'file_not_listed':
        return 'mods.migration.unjudgedReason.fileNotListed' as const;
    }
  }

  function dispositionOptions(row: StrandedRow | NoPlatformBuildRow) {
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

  // ── The modpack block (2026-09-15 pack-origin remedy spec) ─────────────
  // A pack-owned mod cannot be fixed per mod: the pack decides its version.
  // When the instance left the platform its installed pack version targets,
  // the one-click remedy is to re-apply that same version, which writes the
  // pack's Minecraft / loader back.
  const packKnown = $derived(plan?.pack?.kind === 'known' ? plan.pack : null);
  const packUnavailable = $derived(plan?.pack?.kind === 'unavailable' ? plan.pack : null);
  const restoreOffered = $derived(
    (packKnown?.minecraft_differs ?? false) || (packKnown?.loader_differs ?? false),
  );
  // What the restore puts back, named only as far as the version list can
  // tell: exactly the differing axes, and only when the pack version lists a
  // single value for each (the archive pins one; a list of several cannot say
  // which). `null` → the generic "the modpack's platform" copy.
  const restorePlatform = $derived.by(() => {
    if (packKnown === null) return null;
    const { game_versions: mcs, loaders } = packKnown.current;
    const parts: string[] = [];
    if (packKnown.minecraft_differs) {
      if (mcs.length !== 1) return null;
      parts.push(`Minecraft ${mcs[0]}`);
    }
    if (packKnown.loader_differs) {
      if (loaders.length !== 1) return null;
      parts.push(displayLoaderTag(loaders[0]));
    }
    return parts.length > 0 ? parts.join(' · ') : null;
  });
  // The count of other versions filtered by loader only for versions that list
  // one (ATLauncher lists none), so the loader is named only then — never a
  // loader match the count did not check.
  const loadersListed = $derived(packKnown?.alternatives_by_loader ?? false);
  const instancePlatform = $derived(
    packKnown === null
      ? ''
      : loadersListed
        ? `Minecraft ${packKnown.instance_mc} · ${displayLoaderTag(packKnown.instance_loader)}`
        : `Minecraft ${packKnown.instance_mc}`,
  );

  const restoreFlow = createModpackUpdateFlow();
  let restoreInst = $state<InstanceWithStatus | null>(null);
  let restoreError = $state<string | null>(null);
  // Covers the instance lookup before `prepare()` flips the flow to busy, so a
  // double click cannot start two restores.
  let restoreStarting = $state(false);
  // A pack update for this instance already queued or running — possibly one
  // this dialog started before it was closed and reopened. The task registry
  // outlives the component, so a reopened dialog still holds its writes.
  const packUpdateActive = $derived(taskFor({ instanceId })?.kind === 'pack-update');
  // Not `restoreFlow.running`: that leaves out `confirming`, and while the
  // restore's confirm is open the plan's own Apply must stay shut too.
  const restoreBusy = $derived(restoreStarting || restoreFlow.phase !== 'idle' || packUpdateActive);
  // The plan's own Apply and the restore both write to this instance: one at a
  // time. Closing is never blocked by it — a restore can sit queued behind
  // another long task, and the window must not be held hostage meanwhile; only
  // the accidental dismissals (backdrop, Escape) are.
  const writing = $derived(applying || restoreBusy);

  // When a pack update this dialog did NOT start ends, the plan it shows is
  // stale (the instance's platform just changed): load it again. A restore
  // started here reloads in `confirmRestore` instead.
  let startedRestoreHere = false;
  let sawPackUpdate = false;
  $effect(() => {
    if (packUpdateActive) {
      sawPackUpdate = true;
      return;
    }
    if (sawPackUpdate) {
      sawPackUpdate = false;
      if (!startedRestoreHere) void loadPlan();
    }
  });

  // A write outlives the dialog (closing never waits for it). Its follow-ups
  // still report; re-planning into a destroyed dialog would only fire a burst
  // of platform queries nobody sees.
  let alive = true;
  onDestroy(() => {
    alive = false;
  });

  async function startRestore() {
    if (packKnown === null || writing) return;
    const current = packKnown.current;
    restoreStarting = true;
    restoreError = null;
    try {
      const listed = await commands.listInstances();
      if (listed.status === 'error') {
        restoreError = formatError(listed.error);
        return;
      }
      const inst = listed.data.find((i) => i.id === instanceId);
      if (!inst) {
        // The instance disappeared under the dialog: re-planning surfaces the
        // backend's own error for it instead of a guess here.
        void loadPlan();
        return;
      }
      restoreInst = inst;
      await restoreFlow.prepare(inst, current);
      restoreError = restoreFlow.error;
    } finally {
      restoreStarting = false;
    }
  }

  async function confirmRestore(opts: ConfirmOptions) {
    if (restoreInst === null) return;
    // Captured now: the write can outlive this dialog (closing never waits for
    // it), and the follow-up must reach the caller that started it.
    const notifyApplied = onApplied;
    startedRestoreHere = true;
    const ok = await restoreFlow.confirm(restoreInst, opts);
    if (!ok) {
      // `null` when the task was dropped from the queue — not an error.
      restoreError = restoreFlow.error;
      return;
    }
    restoreInst = null;
    // The instance record itself changed (Minecraft / loader): the page owns
    // the instance list, so it refetches on this signal.
    instancesChanged.value++;
    notifyApplied?.();
    if (alive) await loadPlan();
  }

  function cancelRestore() {
    restoreFlow.cancel();
    restoreInst = null;
  }

  function openPackVersionSwitch() {
    if (packKnown === null || writing) return;
    modpacksNav.value = {
      openDrawerForInstance: instanceId,
      openSwitch: loadersListed
        ? { mcFilter: packKnown.instance_mc, loaderFilter: packKnown.instance_loader }
        : { mcFilter: packKnown.instance_mc },
    };
    onClose();
  }

  function strandedReasonText(row: StrandedRow): string {
    if (row.reason.kind === 'loader_too_old' && row.reason.built_for_mc) {
      // A loader-version violation is almost always the jar being built for a
      // different Minecraft version (the loader is bumped every MC version).
      // Name the version it declares, instead of promising a loader update
      // that does not exist.
      return $t('mods.migration.reason.loaderTooOldFor', { mc: row.reason.built_for_mc });
    }
    if (row.reason.kind === 'pack_origin' && restoreOffered) {
      return $t('mods.migration.reason.packOriginCovered');
    }
    return $t(reasonKey(row.reason));
  }

  async function apply() {
    if (!plan || !canApply || writing) return;
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
      // No-release rows ride in the same list: the backend acts on any
      // sha1 + disposition pair and never cross-checks it against the plan.
      // Leaving them out would DROP the user's choice silently.
      stranded: [...plan.stranded, ...plan.no_platform_build].map((row) => ({
        sha1: row.sha1,
        // Undecided rows are kept — a no-op that leaves the jar in place. Never
        // drop a row from the payload, never default it to disable/remove.
        disposition: dispositions.get(row.sha1) ?? 'keep',
      })),
    };
    // Captured now, like the restore: closing does not wait for the write.
    const notifyApplied = onApplied;
    const res = await commands.modsApplyMcMigration(instanceId, selections);
    applying = false;
    if (res.status === 'error') {
      applyError = formatError(res.error);
      return;
    }
    report = res.data;
    phase = 'result';
    notifyApplied?.();
  }
</script>

<Modal
  ariaLabelledby="migration-plan-title"
  {onClose}
  panelClass="w-full max-w-2xl max-h-[85vh] flex flex-col"
  dataTestid="migration-plan-dialog"
  closeOnBackdrop={!writing}
  closeOnEscape={!writing}
>
  <header class="shrink-0 flex items-center justify-between border-b p-4">
    <h2 id="migration-plan-title" class="text-base font-semibold text-primary">
      {$t('mods.migration.title')}
    </h2>
    <CloseButton onClick={onClose} />
  </header>

  <div class="min-h-0 flex-1 overflow-y-auto p-4">
    {#if phase === 'loading'}
      <LoadingPanel
        label={loadingLabel(progress)}
        detail={progress
          ? $t('mods.migration.progress.count', { done: progress.done, total: progress.total })
          : null}
      />
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
        })}{#if plan.no_platform_build.length > 0}
          · {$t('mods.migration.summaryNoRelease', { count: plan.no_platform_build.length })}{/if}
      </p>
      {#if plan.unjudged.length > 0}
        <!-- Native <details> + the shared `.disclosure-caret` rotation
             (DESIGN.md:144,165); same shape inside the shared Modal as
             QuickJoinDialog. Collapsed by default: these mods need nothing
             from the user, they are listed so the count can be looked into. -->
        <details class="mb-3" data-testid="migration-unjudged">
          <summary class="flex cursor-pointer items-center gap-2 text-xs text-muted">
            <span class="disclosure-caret"><Icon name="caret" size={14} /></span>
            {$t('mods.migration.unjudgedLine', { count: plan.unjudged.length })}
          </summary>
          <ul class="mt-2 space-y-1">
            {#each plan.unjudged as row (row.sha1)}
              <li
                class="rounded border border-border-subtle p-2"
                data-testid={`migration-unjudged-row-${row.sha1}`}
              >
                <div class="text-sm text-primary">{row.name}</div>
                <p class="text-xs text-muted">{$t(unjudgedReasonKey(row.reason))}</p>
              </li>
            {/each}
          </ul>
        </details>
      {/if}

      {#if nothingToDo}
        <p class="text-sm text-secondary" data-testid="migration-nothing-to-do">
          {$t('mods.migration.nothingToDo')}
        </p>
      {:else}
        {#if plan.replaceable.length > 0}
          <section class="mb-4" data-testid="migration-replaceable-section">
            <div class="mb-2 flex items-center justify-between gap-2">
              <h3 class="text-xs uppercase tracking-wide text-muted">
                {$t('mods.migration.replaceableHeading', { count: plan.replaceable.length })}
              </h3>
              <SelectAllCheckbox
                allSelected={allReplaceChecked}
                indeterminate={someReplaceChecked && !allReplaceChecked}
                onToggle={toggleAllReplace}
                testid="migration-replace-select-all"
              />
            </div>
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
          <section
            class={plan.no_platform_build.length > 0 ? 'mb-4' : ''}
            data-testid="migration-stranded-section"
          >
            <h3 class="mb-2 text-xs uppercase tracking-wide text-muted">
              {$t('mods.migration.strandedHeading', { count: plan.stranded.length })}
            </h3>
            {#if packKnown}
              <Banner
                tone="warning"
                icon="warning"
                class="mb-3"
                dataTestid="migration-pack-block"
                title={!restoreOffered
                  ? $t('mods.migration.pack.matchesTitle', {
                      pack: packKnown.pack_name,
                      version: packKnown.pack_version,
                    })
                  : restorePlatform === null
                    ? $t('mods.migration.pack.restoreTitleGeneric', {
                        pack: packKnown.pack_name,
                        version: packKnown.pack_version,
                      })
                    : $t('mods.migration.pack.restoreTitle', {
                        pack: packKnown.pack_name,
                        version: packKnown.pack_version,
                        platform: restorePlatform,
                      })}
              >
                {#if restoreOffered}
                  <p class="text-sm">
                    {packKnown.minecraft_differs && packKnown.loader_differs
                      ? $t('mods.migration.pack.restoreBodyBoth', {
                          instanceMc: packKnown.instance_mc,
                        })
                      : packKnown.minecraft_differs
                        ? $t('mods.migration.pack.restoreBodyMc', {
                            instanceMc: packKnown.instance_mc,
                          })
                        : $t('mods.migration.pack.restoreBodyLoader')}
                  </p>
                {/if}
                {#if packKnown.other_versions_for_instance === 0}
                  <p class="text-sm">
                    {$t('mods.migration.pack.noAlternatives', { instancePlatform })}
                  </p>
                {/if}
                <div class="mt-2 flex flex-wrap gap-2">
                  {#if restoreOffered}
                    <BusyButton
                      busy={restoreBusy}
                      disabled={applying}
                      class="btn-warning btn-sm"
                      data-testid="migration-pack-restore"
                      onclick={() => void startRestore()}
                    >
                      {restorePlatform === null
                        ? $t('mods.migration.pack.restoreBtnGeneric')
                        : $t('mods.migration.pack.restoreBtn', { platform: restorePlatform })}
                    </BusyButton>
                  {/if}
                  {#if packKnown.other_versions_for_instance > 0}
                    <button
                      type="button"
                      class="btn-secondary btn-sm"
                      disabled={writing}
                      data-testid="migration-pack-switch"
                      onclick={openPackVersionSwitch}
                    >
                      {$t('mods.migration.pack.switchBtn', {
                        count: packKnown.other_versions_for_instance,
                        instancePlatform,
                      })}
                    </button>
                  {/if}
                </div>
                {#if restoreError}
                  <p class="mt-2 text-sm text-danger" data-testid="migration-pack-error">
                    {restoreError}
                  </p>
                {/if}
              </Banner>
            {:else if packUnavailable}
              <Banner tone="warning" icon="warning" class="mb-3" dataTestid="migration-pack-block">
                <p class="text-sm">
                  {$t('mods.migration.pack.unavailable', { pack: packUnavailable.pack_name })}
                </p>
                <button
                  type="button"
                  class="btn-secondary btn-sm mt-2"
                  disabled={writing}
                  data-testid="migration-pack-retry"
                  onclick={() => void loadPlan()}
                >
                  {$t('mods.migration.pack.retryBtn')}
                </button>
              </Banner>
            {/if}
            <ul class="space-y-2">
              {#each plan.stranded as row (row.sha1)}
                <li
                  class="rounded border border-border-subtle p-2"
                  data-testid={`migration-stranded-row-${row.sha1}`}
                >
                  <div class="text-sm text-primary">{row.name}</div>
                  <p class="mb-1.5 text-xs text-muted">{strandedReasonText(row)}</p>
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

        {#if plan.no_platform_build.length > 0}
          <!-- Deliberately NOT under «needs your decision»: nothing here is
               proven. The intro states both outcomes. -->
          <section data-testid="migration-no-release-section">
            <h3 class="mb-1 text-xs uppercase tracking-wide text-muted">
              {$t('mods.migration.noReleaseHeading', { count: plan.no_platform_build.length })}
            </h3>
            <p class="mb-2 text-xs text-muted">{$t('mods.migration.noReleaseIntro')}</p>
            <ul class="space-y-2">
              {#each plan.no_platform_build as row (row.sha1)}
                <li
                  class="rounded border border-border-subtle p-2"
                  data-testid={`migration-no-release-row-${row.sha1}`}
                >
                  <div class="mb-1.5 text-sm text-primary">{row.name}</div>
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
              disabled={!canApply || restoreBusy}
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

<!-- Stacked above the plan, so rendered after it (DESIGN.md: a stacked modal
     renders after the one it covers). -->
{#if restoreFlow.diff && packKnown}
  <PackPlatformRestoreDialog
    diff={restoreFlow.diff}
    packName={packKnown.pack_name}
    busy={restoreFlow.phase === 'applying'}
    worldCount={restoreFlow.worldCount}
    onCancel={cancelRestore}
    onConfirm={(opts) => void confirmRestore(opts)}
  />
{/if}
