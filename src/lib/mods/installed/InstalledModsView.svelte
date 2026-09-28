<script lang="ts">
  import {
    events,
    type DepViolation,
    type Error as IpcError,
    type LoaderKind,
    type ModSource,
    type ModVersion,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { type InstallOpts, installModWithDeps, updateMod } from '$lib/tasks/adapters/mod-install';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { get } from 'svelte/store';
  import { onDestroy, tick } from 'svelte';
  import { listenUntilDestroyed } from '$lib/ipc/listen';
  import { debounceTrailing } from '$lib/ui/debounce';
  import CurseForgeKeyBanner from '../CurseForgeKeyBanner.svelte';
  import ChangelogModal from '../ChangelogModal.svelte';
  import ModDetailModal from '../ModDetailModal.svelte';
  import CompatWarningDialog from '../CompatWarningDialog.svelte';
  import {
    disableMods,
    enableMods,
    type ModOpScope,
    type ModOpTarget,
    uninstallMods,
  } from '$lib/mods/mod-ops.svelte';
  import PageSizePicker from '../PageSizePicker.svelte';
  import Pagination from '$lib/ui/Pagination.svelte';
  import { browserPrefs } from '../browser-prefs.svelte';
  import { createInstalledData, type Row } from './installed-data.svelte';
  import { createInstalledFilters } from './installed-filters.svelte';
  import { createUpdateCheck } from './update-check.svelte';
  import { createDepGraph } from './dep-graph.svelte';
  import {
    createPreflight,
    hasBlocking,
    installMissing,
    remediatePickedVersion,
    remediateViolation,
    toOverlayKeys,
    violationKey,
  } from '$lib/mods/preflight.svelte';
  import FindAlternativeDialog from '../FindAlternativeDialog.svelte';
  import MigrationPlanDialog from '../MigrationPlanDialog.svelte';
  import { modProjectUrl } from '$lib/mods/project-url';
  import {
    offPlatformFactsOfError,
    offPlatformLabel,
    offPlatformRows,
    type OffPlatformRow,
  } from '$lib/mods/off-platform';
  import { switchTarget } from '$lib/mods/version-switch';
  import { depNameOf, depProjectOf, resolveDepNames } from '$lib/mods/dep-names.svelte';
  import type { DepTreeCtx } from '$lib/mods/dep-node-state';
  import { countFixed, fixAll } from '$lib/mods/fix-all';
  import { SvelteSet } from 'svelte/reactivity';
  import { createInstalledSelection } from './installed-selection.svelte';
  import PreflightPanel from '$lib/mods/PreflightPanel.svelte';
  import { createCompatCheck } from './compat-check.svelte';
  import { isProblem, type ModStatus, statusOf } from './mod-status';
  import { type RowFix, type RowProblem, rowProblemOf } from './row-problem';
  import { modKey, rowDisplayName } from './row-utils';
  import InstalledToolbar from './InstalledToolbar.svelte';
  import BulkActionBar from './BulkActionBar.svelte';
  import InstalledModRow from './InstalledModRow.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import { openExternalHttps } from '$lib/ui/safe-open';

  let {
    instanceId,
    instanceName = null,
    mcVersion,
    loader,
    requestedFilter = null,
    onFilterApplied = () => {},
    onBrowseFor = (_q: string) => {},
  }: {
    instanceId: string | null;
    // Named in a restore toast once the user has switched profiles (spec §6.1).
    instanceName?: string | null;
    mcVersion: string | null;
    loader: LoaderKind | null;
    // A status view asked for by a deep-link (the Overview's attention item →
    // «Проблемы», the one problem view). Applied once, then cleared by the
    // parent so an in-tab click is never hijacked afterwards.
    requestedFilter?: 'issues' | null;
    onFilterApplied?: () => void;
    onBrowseFor?: (query: string) => void;
  } = $props();

  // --- composables (creation order matters; thunks keep cross-refs lazy) ---
  const data = createInstalledData(() => instanceId);
  const updates = createUpdateCheck(() => instanceId, data.refresh);
  const compat = createCompatCheck(
    () => instanceId,
    () => mcVersion,
    () => loader,
  );
  // Declared before `filters` because `hasIssue` reads it: the pre-flight is the
  // ONLY source of "this mod stops the game". The graph reports what the
  // platform was told; only the pre-flight reads the descriptor the loader
  // enforces. A mod is blocking iff it is the dependent in a violation.
  const preflight = createPreflight(() => instanceId);
  const outOfRangeKeys = $derived(toOverlayKeys(preflight.report ?? { violations: [] }));
  // Blocking = the gate's predicate (`hasBlocking`), one predicate for the
  // issues chip, the row line and the «What stops the game» panel (plan A17):
  // while a self-completing pack still has files to download, its violations
  // are advisory and nothing here says the game won't start. Grouped by
  // dependent, in report order.
  const blockingViolations = $derived(
    preflight.report && hasBlocking(preflight.report) ? preflight.report.violations : [],
  );
  const violationsBySha = $derived.by(() => {
    const m = new Map<string, DepViolation[]>();
    for (const v of blockingViolations) {
      m.set(v.dependent_sha1, [...(m.get(v.dependent_sha1) ?? []), v]);
    }
    return m;
  });
  // Enabled mods on a Vanilla instance are dead weight (spec D9) — drives
  // the instance-level banner above the list.
  const enabledModsCount = $derived(data.rows.filter((r) => r.installed.enabled).length);

  // Display names by sha1 (rowDisplayName) — every guarded operation names mods this way, and
  // captures instance + profile when it starts (spec §6.1 "Instance binding").
  const nameBySha = $derived(
    new Map<string, string>(data.rows.map((r) => [r.installed.sha1, rowDisplayName(r)])),
  );
  function opScope(id: string): ModOpScope {
    return { instanceId: id, profileName: instanceName, nameOf: (sha1) => nameBySha.get(sha1) };
  }

  const rowBySha = $derived(new Map<string, Row>(data.rows.map((r) => [r.installed.sha1, r])));
  // The compat hint behind a warning — only for a mod compat counts as
  // incompatible, the membership the old «Несовместимые» chip counted.
  const compatHintOf = (sha1: string) =>
    compat.incompatibleShas.has(sha1) ? compat.hintFor(sha1) : null;
  // One status per row (mod-status.ts); the «Проблемы» chip is blocking ∪
  // warning. `held` stays false until holds are wired in — a hold never changes
  // a problem level, so the chip is right either way.
  const statusBySha = $derived(
    new Map<string, ModStatus>(
      data.rows.map((r) => [
        r.installed.sha1,
        statusOf({
          enabled: r.installed.enabled,
          violations: violationsBySha.get(r.installed.sha1) ?? [],
          compat: compatHintOf(r.installed.sha1),
          update: updates.updateChecks.get(r.installed.sha1)?.state ?? null,
          held: false,
        }),
      ]),
    ),
  );
  // The chip turns red only while a row IS red — the same statuses it counts.
  const anyBlocking = $derived([...statusBySha.values()].some((s) => s.level === 'blocking'));
  // The dependency graph is rebuilt on install and removal, not on a toggle,
  // and the backend roots it at the ENABLED mods only — so after a toggle it is
  // stale. What the rows can tell is taken from the rows:
  // - a mod switched off since requires nothing at load time: only roots that
  //   are enabled NOW count (`requiredByCount`);
  // - a mod switched on since has no root yet, so what it requires is unknown:
  //   the graph views are no fact until the graph knows every enabled platform
  //   mod (`graphCoversEnabled`) — a library it needs could read as unused.
  const requiredByCount = (r: Row | undefined): number =>
    (deps.requiredBy.get(r?.installed.project_id ?? '') ?? []).filter(
      (e) => rowBySha.get(e.sha1)?.installed.enabled === true,
    ).length;
  const graphCoversEnabled = (): boolean =>
    deps.graph !== null &&
    data.rows.every(
      ({ installed: m }) => !m.enabled || !m.source || !m.project_id || deps.rootBySha.has(m.sha1),
    );

  const filters = createInstalledFilters(
    () => data.rows,
    (r) => ({
      id: r.installed.sha1,
      name: rowDisplayName(r),
      enabled: r.installed.enabled,
      sortKey: r.installed.installed_at,
      source: r.installed.source,
      searchTerms: [r.installed.filename, r.summary?.slug ?? null],
    }),
    {
      isUpdatable: (id) => updates.updatableShas.has(id),
      hasIssue: (id) => isProblem(statusBySha.get(id)),
      // Enabled, like the graph's own "installed": a switched-off jar satisfies nobody.
      isNeeded: (id) => {
        const r = rowBySha.get(id);
        return !!r && r.installed.enabled && requiredByCount(r) > 0;
      },
      // `library === true` only: `null` (a source that cannot tell) is never a library.
      isUnusedLibrary: (id) => {
        const r = rowBySha.get(id);
        return (
          !!r && r.installed.enabled && r.summary?.library === true && requiredByCount(r) === 0
        );
      },
      // `deps` is created below; these thunks only run once counts are read.
      graphReady: graphCoversEnabled,
    },
    // A status count of 0 is "not known yet" until the rows AND the pre-flight
    // have answered (a report or an error — either settles it). `refresh()`
    // sets `loading` synchronously before its first await and the pre-flight
    // starts without a report, so this is false when the filters' auto-reset
    // effect first runs on mount. The compat half is not waited for: its scan is
    // shared with the Overview, where the deep-link comes from, so it has
    // already answered for this profile by then.
    () => !data.loading && (preflight.report !== null || preflight.error !== null),
  );
  const deps = createDepGraph(
    () => instanceId,
    () => data.rows,
    {
      getMcVersion: () => mcVersion,
      getLoader: () => loader,
      refresh: data.refresh,
      getFiltered: () => filters.filtered,
      setPage: (n) => (filters.page = n),
      getPageSize: () => filters.pageSize,
    },
  );
  const selection = createInstalledSelection(
    () => filters.filtered,
    () => instanceId,
    data.refresh,
    () => updates.updateChecks,
    deps.invalidateGraph,
    opScope,
  );

  // Per-row pre-flight remediation state, keyed by violationKey. `busy` shows a
  // spinner on the row; `deadEnd` flips it to the no-satisfying affordances
  // (open mod page / find alternative). The picker + find-alternative dialogs
  // are driven by the *Violation holders below.
  let preflightBusy = $state(new SvelteSet<string>());
  let preflightDeadEnd = $state(new SvelteSet<string>());
  let pickerViolation = $state<DepViolation | null>(null);
  let findAltViolation = $state<DepViolation | null>(null);

  // The safety net (2026-09-20 spec, D6). The backend refuses a build the
  // platform does not list for this instance unless the user said yes. When
  // that refusal reaches this view — the modal could not tell, or the pick
  // never went through it — ASK, from the error's own fields; a failure toast
  // would describe something no retry can fix. The consent is never stored:
  // `proceed` re-runs the one action the user was just asked about.
  let offPlatformPrompt = $state<{ rows: OffPlatformRow[]; proceed: () => void } | null>(null);

  function askOffPlatform(err: IpcError, v: ModVersion, proceed: () => void): boolean {
    const facts = offPlatformFactsOfError(err);
    if (!facts) return false;
    offPlatformPrompt = {
      rows: offPlatformRows(offPlatformLabel(v.name, v.version_number), facts, get(t)),
      proceed,
    };
    return true;
  }

  // Human names for the dependencies in the current report live in one module
  // store (dep-names.svelte.ts), keyed by instance + (dependent, dep id) and
  // readable by every surface. This tab may spend a network round on them; the
  // store asks only for names it does not have, reads itself untracked (its
  // answers never re-run this effect) and never throws. Anything unresolved
  // stays absent and the panel falls back to the raw loader id.
  $effect(() => {
    const report = preflight.report;
    const id = instanceId;
    if (!report || !id) return;
    void resolveDepNames(id, report);
  });
  // A dependency's display name. When the report names the provider's jar — the
  // disabled one to switch back on, or the enabled one a range points at — that
  // jar is a row here, and its own name is the thing the player can act on;
  // otherwise the dependent-scoped store (spec §5.3), else the raw loader id.
  function depName(v: DepViolation): string {
    const own = v.provider_sha1 ? nameBySha.get(v.provider_sha1) : undefined;
    return own ?? depNameOf(instanceId, v.dependent_sha1, v.dep_id) ?? v.dep_id;
  }

  // What the dependency trees need to say what the loader does (spec §6.3). The FULL report, not
  // the blocking subset: "without it the game won't start" stays true while a pack is still
  // completing itself. A tree knows a dependency by its project only, so its «Enable» looks the
  // disabled jar up by (source, project id) and takes the row's own guarded path.
  const shaByKey = (enabled: boolean) =>
    new Map<string, string>(
      data.rows
        .filter((r) => r.installed.enabled === enabled)
        .map((r) => [
          modKey(r.installed.source, r.installed.project_id, r.installed.sha1),
          r.installed.sha1,
        ]),
    );
  const enabledShaByKey = $derived(shaByKey(true));
  const disabledShaByKey = $derived(shaByKey(false));
  const treeCtx: DepTreeCtx = {
    get report() {
      return preflight.report;
    },
    projectOf: (sha1, depId) => depProjectOf(instanceId, sha1, depId),
    enabledShaOf: (key) => enabledShaByKey.get(key) ?? null,
    onEnable: (node) => {
      const sha1 = disabledShaByKey.get(`${node.source}:${node.project_id}`);
      // Switched on or removed since the graph was built: nothing is off to switch on.
      if (!sha1) return;
      void setEnabled([{ sha1, name: nameBySha.get(sha1) ?? node.name }], true);
    },
  };

  // Reset per-row remediation state on instance switch. The keys are dep-based
  // (dependent_sha1:dep_id), not instance-scoped, so a stale busy spinner or
  // dead-end could otherwise bleed onto a same-named dep in another instance.
  $effect(() => {
    void instanceId;
    preflightBusy.clear();
    preflightDeadEnd.clear();
    pickerViolation = null;
    findAltViolation = null;
    offPlatformPrompt = null;
  });

  async function refreshAfterRemediate(): Promise<void> {
    preflight.invalidate();
    deps.invalidateGraph();
    await data.refresh();
  }

  // Smart one-click update: install the newest version that satisfies the dep
  // range AND the instance MC/loader. On a no-satisfying dead-end the row flips
  // to the open-page / find-alternative affordances instead of a useless retry.
  const onPreflightUpdate = async (v: DepViolation): Promise<void> => {
    if (!instanceId || !mcVersion || !loader) return;
    const key = violationKey(v);
    preflightBusy.add(key);
    // finally clears the busy key even if an IPC call throws (bridge teardown),
    // so a row can never get stuck showing a spinner.
    try {
      const result = await remediateViolation(instanceId, v, mcVersion, loader);
      if (result.ok) {
        preflightDeadEnd.delete(key);
        pushSuccess(
          get(t)('mods.preflight.installedVersion', {
            dep: depName(v),
            version: result.installedVersion ?? '',
          }),
        );
        await refreshAfterRemediate();
      } else if (result.reason === 'no-satisfying') {
        preflightDeadEnd.add(key);
      } else {
        pushWarning(get(t)('mods.browse.toastInstallFailed'));
      }
    } finally {
      preflightBusy.delete(key);
    }
  };

  // Open the dependency's version list so the user can install any version
  // (including a downgrade) — routed in place via remediatePickedVersion.
  const onPreflightChooseVersion = (v: DepViolation): void => {
    pickerViolation = v;
  };

  // Open the find-alternative search for a dependency with no satisfying version.
  const onPreflightFindAlternative = (v: DepViolation): void => {
    findAltViolation = v;
  };

  // Open the dependency's platform page in the browser.
  const onPreflightOpenModPage = (v: DepViolation): void => {
    const ref = v.provider_project;
    if (!ref) return;
    const slugOrId = ref.source === 'modrinth' ? ref.project_id : String(ref.mod_id);
    void openExternalHttps(modProjectUrl(ref.source, slugOrId));
  };

  // Install a user-chosen version from the picker (manual pick / downgrade).
  // `opts` carries the modal's confirmation of a build the platform does not
  // list for this instance.
  const onPreflightPickInstall = async (
    chosen: ModVersion,
    opts: InstallOpts = {},
  ): Promise<void> => {
    if (!instanceId || !pickerViolation) return;
    await runPickedInstall(instanceId, pickerViolation, chosen, opts);
  };

  async function runPickedInstall(
    id: string,
    v: DepViolation,
    chosen: ModVersion,
    opts: InstallOpts,
  ): Promise<void> {
    // Another build of the same project can be installed although the
    // preflight has no `provider_sha1` for it: the pick is then a version
    // switch, and a switch never installs beside the old jar.
    const existing =
      data.rows.find(
        (x) => x.installed.source === chosen.source && x.installed.project_id === chosen.project_id,
      )?.installed ?? null;
    const r = await remediatePickedVersion(id, v, chosen, {
      ...opts,
      installedSha1: switchTarget(existing, chosen),
    });
    if (r.ok) {
      preflightDeadEnd.delete(violationKey(v));
      pickerViolation = null;
      pushSuccess(
        get(t)('mods.preflight.installedVersion', {
          dep: depName(v),
          version: r.installedVersion ?? '',
        }),
      );
      await refreshAfterRemediate();
      return;
    }
    if (
      r.error &&
      askOffPlatform(
        r.error,
        chosen,
        () => void runPickedInstall(id, v, chosen, { allowOffPlatform: true }),
      )
    ) {
      return;
    }
    pushWarning(get(t)('mods.browse.toastInstallFailed'));
  }

  // One-click install of a missing required dependency from the pre-flight
  // panel. Resolves the dep by its loader mod-id and installs it; on success
  // the panel + graph refresh. When the dep can't be auto-resolved the helper
  // returns an open_search outcome — hand the query up to the Add-ons shell so
  // it switches to Browse with the search pre-filled.
  const onInstallMissingDep = async (v: DepViolation): Promise<void> => {
    if (!instanceId) return;
    const outcome = await installMissing(instanceId, v.dependent_sha1, v.dep_id);
    if (outcome.kind === 'installed') {
      pushSuccess(get(t)('mods.browse.toastInstalledMod', { name: outcome.name }));
      preflight.invalidate();
      deps.invalidateGraph();
      await data.refresh();
    } else {
      pushWarning(get(t)('mods.preflight.installSearchFallback', { dep: depName(v) }));
      onBrowseFor(outcome.query);
    }
  };

  // A find-alternative install resolves the original violation (the alternative
  // now provides the dep) — clear its dead-end state and refresh. The dialog
  // shows its own success toast.
  const onPreflightAltInstalled = async (): Promise<void> => {
    if (findAltViolation) preflightDeadEnd.delete(violationKey(findAltViolation));
    findAltViolation = null;
    await refreshAfterRemediate();
  };

  // `required_disabled` → switch the provider back on through mod-ops (guarded:
  // it asks when the provider has disabled requirements of its own). The
  // modToggle event re-runs the pre-flight. `depName` is the provider row's name.
  function enableProvider(v: DepViolation): Promise<void> {
    const sha1 = v.provider_sha1;
    if (!sha1) return Promise.resolve();
    return setEnabled([{ sha1, name: depName(v) }], true);
  }

  // Bring a violation's panel row into view: the row line's «and N more», and
  // its «Fix…» until the version-fix planner has a flow of its own.
  async function revealInPanel(v: DepViolation): Promise<void> {
    await tick();
    if (typeof document === 'undefined') return;
    const key = violationKey(v);
    // Matched by value, not by a selector: a key is data (a sha1 and a mod id).
    const row = [...document.querySelectorAll<HTMLElement>('[data-violation-key]')].find(
      (el) => el.dataset.violationKey === key,
    );
    row?.scrollIntoView?.({ behavior: 'smooth', block: 'center' });
  }

  // ↗ from a panel row. A dependent is always in the problem view, so a search
  // or chip that hides it is cleared rather than the jump silently doing nothing.
  async function jumpToDependent(v: DepViolation): Promise<void> {
    if (await deps.jumpToSha1(v.dependent_sha1)) return;
    filters.filter = '';
    filters.viewFilter = 'issues';
    await tick();
    await deps.jumpToSha1(v.dependent_sha1);
  }

  // «Fix all (N)»: the Play gate's repair (fix-all.ts), then a FRESH pre-flight
  // — only it says which rows are gone — and one toast «Fixed N of M». The
  // profile it ran for is captured and named when the user has moved on.
  let fixAllBusy = $state(false);
  async function runFixAll(): Promise<void> {
    const id = instanceId;
    const name = instanceName;
    const report = preflight.report;
    if (!id || !report || fixAllBusy) return;
    fixAllBusy = true;
    try {
      const { attempted } = await fixAll(id, report);
      const after = await preflight.check(id);
      deps.invalidateGraph();
      await data.refresh();
      const where =
        instanceId !== id && name ? [get(t)('mods.ops.restore.inProfile', { profile: name })] : [];
      if (after.status !== 'ok') {
        pushWarning(get(t)('mods.preflight.checkFailed'), [formatError(after.error), ...where]);
        return;
      }
      const fixed = countFixed(attempted, after.data);
      const title = get(t)('mods.preflight.gateFixed', { fixed, total: attempted.length });
      if (fixed === attempted.length) pushSuccess(title, where);
      else pushWarning(title, where);
    } catch (e) {
      // The bridge failed on the re-check (the repair's own steps never throw):
      // what is left is unknown until the next pre-flight.
      pushWarning(get(t)('mods.preflight.checkFailed'), [
        e instanceof Error ? e.message : String(e),
      ]);
    } finally {
      fixAllBusy = false;
    }
  }

  // The row's second line, from the row's one status (its level, its ranked
  // reasons and the fix the status chose) — never a second ranking.
  function problemOf(row: Row): RowProblem | null {
    const status = statusBySha.get(row.installed.sha1);
    if (!status) return null;
    return rowProblemOf(status, {
      t: $t,
      depName,
      loader,
      mc: mcVersion,
      canChooseVersion: !!(row.installed.source && row.installed.project_id),
    });
  }

  function onRowFix(row: Row, fix: RowFix): void {
    switch (fix.kind) {
      case 'enable':
        void enableProvider(fix.violation);
        return;
      case 'install':
        void onInstallMissingDep(fix.violation);
        return;
      case 'plan':
        void revealInPanel(fix.violation);
        return;
      case 'choose_version':
        // Another build of THIS mod: its own versions, in the detail modal.
        if (row.installed.source && row.installed.project_id)
          openDetailMod(row.installed.source as ModSource, row.installed.project_id);
        return;
    }
  }

  // Independent per-instance page size, persisted under its own key.
  $effect(() => {
    filters.pageSize = browserPrefs.installedPageSize;
  });

  // Drive the compat pipeline: re-scan whenever the instance / mc / loader
  // changes. The composable owns no self-effect (kept directly unit-testable);
  // this is its single reactive trigger, plus the mod add/remove/toggle handlers
  // in onMount. The composable's generation guard makes a rapid switch supersede
  // any in-flight scan from the previous instance.
  $effect(() => {
    void instanceId;
    void mcVersion;
    void loader;
    void compat.runOfflineScan();
  });

  // Apply a deep-linked status view once. The scan that populates the chip may
  // still be in flight when this lands, so it is applied unconditionally — the
  // composable's existing auto-reset drops back to `all` if the count really
  // is zero, which keeps a stale link from stranding the user on an empty list.
  $effect(() => {
    if (requestedFilter === null) return;
    filters.viewFilter = requestedFilter;
    onFilterApplied();
  });

  // Single-row ops (toggle/uninstall/detail install) live in the shell, so they
  // need their own busy flag folded into the aggregate — otherwise the toolbar
  // and bulk bar stay clickable mid-IPC (the monolith gated them via `busy`).
  let shellBusy = $state(false);

  const busy = $derived(shellBusy || fixAllBusy || selection.busy || deps.busy || updates.busy);
  const error = $derived(data.error ?? deps.error ?? updates.error ?? selection.error);

  // Detail modal can target ANY mod by (source, project_id): the row's own mod,
  // an installed dependency, or a not-yet-installed dependency. Install resolves
  // to a swap when a different version of the same project is already installed,
  // else a fresh install.
  let detail = $state<{ source: ModSource; projectId: string } | null>(null);
  function openDetailMod(source: ModSource, projectId: string) {
    detail = { source, projectId };
  }

  // MigrationPlanDialog: instance-wide, opened from any violated row's "Fix"
  // button. Not scoped to the row that opened it — the plan judges every
  // installed mod at once.
  let migrationDialogOpen = $state(false);

  // Cumulative changelog for a pending mod update. The row exposes the button
  // only when an update is available from a supported source; this builds the
  // (installed → target) request from the update-check result.
  let changelogReq = $state<{
    source: ModSource;
    projectId: string;
    title: string;
    target: string;
    base: string | null;
  } | null>(null);
  function openChangelog(row: Row) {
    const c = updates.updateChecks.get(row.installed.sha1);
    if (!c || c.state.kind !== 'update_available') return;
    const { source, project_id, version_id } = row.installed;
    if (!source || !project_id) return;
    changelogReq = {
      source,
      projectId: project_id,
      title: `${rowDisplayName(row)} ${row.installed.version_number ?? ''} → ${c.state.target.version_number}`,
      target: c.state.target.version_id,
      base: version_id,
    };
  }
  // The installed build of the project the detail modal shows: its version id
  // AND its bytes. `enrich` leaves `version_id` null for exactly the mods whose
  // platform tags disagree with the instance, and those are recognised by sha1.
  const detailInstalled = $derived.by(() => {
    if (!detail) return null;
    const r = data.rows.find(
      (x) => x.installed.source === detail!.source && x.installed.project_id === detail!.projectId,
    );
    return r?.installed ?? null;
  });

  async function installDetailVersion(v: ModVersion, opts: InstallOpts = {}) {
    if (!instanceId || !detail) return;
    const existing = data.rows.find(
      (x) => x.installed.source === detail!.source && x.installed.project_id === detail!.projectId,
    );
    const name = existing?.summary?.name ?? existing?.installed.name ?? v.name;
    detail = null;
    await runVersionInstall(
      instanceId,
      name,
      switchTarget(existing?.installed ?? null, v),
      v,
      opts,
    );
  }

  // `oldSha1` set = a version switch: ONE command that downloads the new build
  // before it removes the old one. It used to be a removal and then an install,
  // as two IPC calls — any failure of the second (resolution, download,
  // verification) left the user with no version of the mod at all. Like
  // «Update», a switch does not offer optional dependencies.
  // (The removal command is deliberately not NAMED here:
  // tests/installed-version-switch-wiring.test.ts greps this handler for it.)
  async function runVersionInstall(
    id: string,
    name: string,
    oldSha1: string | null,
    v: ModVersion,
    opts: InstallOpts,
  ) {
    shellBusy = true;
    data.error = null;
    const res = oldSha1
      ? await updateMod(id, name, oldSha1, v, opts)
      : await installModWithDeps(
          id,
          name,
          { source: v.source, project_id: v.project_id, version_id: v.version_id },
          [],
          opts,
        );
    shellBusy = false;
    if (res.status === 'error') {
      // Refused before anything was touched — nothing to refresh.
      if (
        askOffPlatform(
          res.error,
          v,
          () => void runVersionInstall(id, name, oldSha1, v, { allowOffPlatform: true }),
        )
      ) {
        return;
      }
      pushWarning(get(t)('mods.browse.toastInstallFailed'), [formatError(res.error)]);
    } else {
      pushSuccess(get(t)('mods.browse.toastInstalledMod', { name }));
    }
    deps.invalidateGraph();
    preflight.invalidate();
    await data.refresh();
  }

  // Enable, disable and removal go through the guarded path (mod-ops): the mods that need this
  // one, or the disabled mods it needs, are asked about first; failures are toasted there.
  async function setEnabled(targets: ModOpTarget[], enable: boolean): Promise<void> {
    if (!instanceId) return;
    data.error = null;
    shellBusy = true;
    try {
      const scope = opScope(instanceId);
      const outcome = enable ? await enableMods(scope, targets) : await disableMods(scope, targets);
      if (outcome !== 'cancelled') await data.refresh();
    } finally {
      shellBusy = false;
    }
  }
  function toggle(row: Row): Promise<void> {
    return setEnabled(
      [{ sha1: row.installed.sha1, name: rowDisplayName(row) }],
      !row.installed.enabled,
    );
  }
  async function uninstall(row: Row) {
    if (!instanceId) return;
    const target = [{ sha1: row.installed.sha1, name: rowDisplayName(row) }];
    data.error = null;
    shellBusy = true;
    try {
      if ((await uninstallMods(opScope(instanceId), target)) !== 'cancelled') {
        await data.refresh();
        deps.reloadGraph();
        preflight.invalidate();
      }
    } finally {
      shellBusy = false;
    }
  }

  // Bulk update: apply, then clear the now-stale update-check state so badges
  // don't linger (the selection composable owns the update IPC but not the
  // update-check cache, which lives in the update-check composable).
  async function bulkUpdate() {
    await selection.bulkUpdate();
    updates.clearChecks();
  }

  // Event listeners (belt-and-suspenders; also call refresh directly). The
  // compat composable's effect only re-runs on instance/mc/loader change, so we
  // also re-scan on mod add/remove/toggle here — otherwise a freshly installed
  // mod's incompatibility chip would not appear until the next instance switch.
  //
  // `force` is load-bearing, not defensive: the shared store keys on
  // (instance, mc, loader), and none of these events changes that key, so an
  // unforced call is deduplicated away and the re-scan silently does nothing.
  // That is what happened between #332 and this fix — the PR that gave the two
  // surfaces one store also disabled the trigger that kept it fresh.
  //
  // Registration/teardown is race-safe via listenUntilDestroyed (the pattern
  // was born here and is now the shared helper). Handlers are debounced: a
  // with-deps install emits one event per jar, and each un-coalesced event
  // used to trigger a full refresh + preflight resolve + compat scan.
  const debouncedSetChanged = debounceTrailing(() => {
    void data.refresh();
    deps.reloadGraph();
    preflight.invalidate();
    void compat.runOfflineScan({ force: true });
  }, 150);
  const debouncedToggle = debounceTrailing(() => {
    void data.refresh();
    preflight.invalidate();
    void compat.runOfflineScan({ force: true });
  }, 150);
  // Something OTHER than us wrote into mods/. Refresh everything derived from
  // the mod list — and deliberately NOT the list. `mods_list_installed` is what
  // emits this event, so refreshing it here would feed the handler its own
  // trigger. Whatever call produced the event already returned the reconciled
  // list to its caller, so the rows are current without our help.
  const debouncedExternalChange = debounceTrailing(() => {
    deps.reloadGraph();
    preflight.invalidate();
    void compat.runOfflineScan({ force: true });
  }, 150);
  listenUntilDestroyed([
    events.modInstalled.listen(debouncedSetChanged.call),
    events.modUninstalled.listen(debouncedSetChanged.call),
    events.modToggle.listen(debouncedToggle.call),
    events.modsReconciled.listen(debouncedExternalChange.call),
  ]);
  onDestroy(() => {
    debouncedSetChanged.cancel();
    debouncedToggle.cancel();
    debouncedExternalChange.cancel();
    data.dispose();
    filters.dispose();
    updates.dispose();
    deps.dispose();
    preflight.dispose();
    selection.dispose();
    compat.dispose();
  });
</script>

<div class="p-3">
  <InstalledToolbar
    counts={filters.counts}
    bind:filter={filters.filter}
    bind:sortBy={filters.sortBy}
    bind:viewFilter={filters.viewFilter}
    {busy}
    checking={updates.checking}
    graphLoading={deps.graphLoading}
    updateCount={updates.updateCount}
    onCheckUpdates={updates.checkUpdates}
    onRecheckDeps={deps.recheckDeps}
    onUpdateAll={updates.updateAll}
    checkingCompat={compat.checking}
    onCheckCompat={compat.runLiveCheck}
    issuesTone={anyBlocking ? 'danger' : 'warning'}
  />

  {#if error}
    <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2 mb-2">
      {error}
    </div>
  {/if}
  {#if updates.showCfBanner}
    <CurseForgeKeyBanner />
  {/if}

  {#if loader === 'vanilla' && enabledModsCount > 0}
    <!-- Instance-level condition, not a per-jar verdict (spec D9): Vanilla
         has no family for compat_verdict to mismatch, yet it loads NO mods
         at all. No fix button — the fix is picking a loader in Manage. -->
    <div
      class="rounded-lg border border-warning-text bg-warning-bg px-3 py-2 text-sm text-warning-text mb-2"
      data-testid="vanilla-mods-banner"
    >
      {$t('instance.integrity.vanillaModsWarning', { count: enabledModsCount })}
    </div>
  {/if}

  <PreflightPanel
    report={preflight.report}
    {instanceId}
    {depName}
    onUpdate={onPreflightUpdate}
    onInstallMissing={onInstallMissingDep}
    onEnableProvider={enableProvider}
    onJumpToDependent={jumpToDependent}
    onFixAll={runFixAll}
    {fixAllBusy}
    onChooseVersion={onPreflightChooseVersion}
    onFindAlternative={onPreflightFindAlternative}
    onOpenModPage={onPreflightOpenModPage}
    onMigrate={() => (migrationDialogOpen = true)}
    migrateCount={compat.incompatibleCount}
    busyKeys={preflightBusy}
    deadEndKeys={preflightDeadEnd}
  />

  {#if !instanceId}
    <div class="text-placeholder text-sm py-8 text-center">
      {$t('mods.installed.pickInstanceFirst')}
    </div>
  {:else if data.loading && data.rows.length === 0}
    <LoadingPanel label={$t('mods.installed.loading')} />
  {:else if data.rows.length === 0}
    <div class="text-placeholder text-sm py-8 text-center">{$t('mods.installed.empty')}</div>
  {:else}
    <div class="border border-border-subtle rounded overflow-hidden">
      <BulkActionBar
        allSelected={selection.allSelected}
        selectedCount={selection.selected.size}
        indeterminate={selection.selected.size > 0 && !selection.allSelected}
        {busy}
        busyAction={selection.busyAction}
        canUpdate={selection.selectedUpdatable.length > 0}
        onToggleAll={selection.toggleSelectAll}
        onEnable={() => selection.bulkSetEnabled(true)}
        onDisable={() => selection.bulkSetEnabled(false)}
        onUpdate={bulkUpdate}
        onUninstall={selection.requestBulkUninstall}
        onClear={selection.clear}
      />
      {#each filters.paged as row (row.installed.sha1)}
        {@const rowKey = modKey(row.installed.source, row.installed.project_id, row.installed.sha1)}
        {@const root = deps.rootBySha.get(row.installed.sha1)}
        {@const counts = deps.depCounts(root)}
        {@const reqBy = deps.requiredBy.get(row.installed.project_id ?? '') ?? []}
        <InstalledModRow
          summary={row.summary}
          installed={row.installed}
          {rowKey}
          {root}
          requiredBy={reqBy}
          depTotal={counts.total}
          problem={problemOf(row)}
          expanded={deps.expanded.has(row.installed.sha1)}
          graphLoading={deps.graphLoading}
          hoveredKey={deps.hoveredKey}
          updateState={updates.updateChecks.get(row.installed.sha1)?.state ?? null}
          checking={updates.checking}
          packChip={data.packSummary && data.packSummary.mod_shas.includes(row.installed.sha1)
            ? data.packSummary.project_name
            : null}
          selected={selection.selected.has(row.installed.sha1)}
          {outOfRangeKeys}
          {treeCtx}
          onToggleExpand={() => deps.toggleExpand(row.installed.sha1)}
          onHover={(k) => (deps.hoveredKey = k)}
          onOpenDetail={() => {
            if (row.installed.source && row.installed.project_id)
              openDetailMod(row.installed.source as ModSource, row.installed.project_id);
          }}
          onOpenDetailMod={openDetailMod}
          onToggle={() => toggle(row)}
          onUninstall={() => uninstall(row)}
          onUpdate={() => updates.updateOne(row.installed)}
          onShowChangelog={() => openChangelog(row)}
          onSelectChange={(c) => selection.toggleSelect(row.installed.sha1, c)}
          onInstallDep={deps.installDepNode}
          onJump={deps.jumpToMod}
          onProblemFix={(fix) => onRowFix(row, fix)}
          onRevealProblems={() => {
            const first = violationsBySha.get(row.installed.sha1)?.[0];
            if (first) void revealInPanel(first);
          }}
        />
      {/each}
    </div>

    <!-- Pagination footer — unified with Browse/Modpacks (Steam-style). -->
    <div class="sticky bottom-0 z-10 bg-base border-t border-border-subtle">
      <Pagination
        page={filters.page}
        pageCount={filters.pageCount}
        onPage={(n) => (filters.page = n)}
      >
        {#snippet end()}
          <PageSizePicker prefsKey="installedPageSize" />
        {/snippet}
      </Pagination>
    </div>
  {/if}

  {#if detail && instanceId}
    <ModDetailModal
      source={detail.source}
      projectId={detail.projectId}
      {mcVersion}
      {loader}
      installedVersionId={detailInstalled?.version_id ?? null}
      installedSha1={detailInstalled?.sha1 ?? null}
      onClose={() => (detail = null)}
      onInstall={installDetailVersion}
    />
  {/if}

  {#if changelogReq}
    <ChangelogModal
      source={changelogReq.source}
      projectId={changelogReq.projectId}
      title={changelogReq.title}
      targetVersionId={changelogReq.target}
      baseVersionId={changelogReq.base}
      onClose={() => (changelogReq = null)}
    />
  {/if}

  {#if pickerViolation && pickerViolation.provider_project && instanceId}
    {@const pp = pickerViolation.provider_project}
    <ModDetailModal
      source={pp.source}
      projectId={pp.source === 'modrinth' ? pp.project_id : String(pp.mod_id)}
      kind="mod"
      {mcVersion}
      {loader}
      installedVersionId={null}
      installedSha1={pickerViolation.provider_sha1}
      needed={pickerViolation.needed}
      family={pickerViolation.family}
      onClose={() => (pickerViolation = null)}
      onInstall={onPreflightPickInstall}
    />
  {/if}

  {#if findAltViolation && instanceId && mcVersion && loader}
    <FindAlternativeDialog
      modName={depName(findAltViolation)}
      {mcVersion}
      {loader}
      {instanceId}
      onClose={() => (findAltViolation = null)}
      onInstalled={onPreflightAltInstalled}
    />
  {/if}

  {#if offPlatformPrompt}
    <CompatWarningDialog
      rows={offPlatformPrompt.rows}
      onConfirm={() => {
        const proceed = offPlatformPrompt?.proceed;
        offPlatformPrompt = null;
        proceed?.();
      }}
      onCancel={() => (offPlatformPrompt = null)}
    />
  {/if}

  {#if migrationDialogOpen && instanceId}
    <MigrationPlanDialog
      {instanceId}
      onClose={() => (migrationDialogOpen = false)}
      onPlanLoaded={() => void compat.runLiveCheck()}
      onApplied={() => {
        // Same refresh set the explicit uninstall/toggle handlers already
        // trigger — belt-and-suspenders alongside the mod-installed /
        // mod-uninstalled / mod-toggle events the apply command re-emits per
        // action (see the debounced event listeners below).
        void data.refresh();
        deps.reloadGraph();
        preflight.invalidate();
        void compat.runOfflineScan({ force: true });
      }}
    />
  {/if}
</div>
