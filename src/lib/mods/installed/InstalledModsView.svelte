<script lang="ts">
  import {
    commands,
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
  import { pushInfo, pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { get } from 'svelte/store';
  import { onDestroy, type Snippet, tick, untrack } from 'svelte';
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
    modWriteReason,
    uninstallMods,
  } from '$lib/mods/mod-ops.svelte';
  import PageSizePicker from '../PageSizePicker.svelte';
  import Pagination from '$lib/ui/Pagination.svelte';
  import { browserPrefs, PAGE_SIZES } from '../browser-prefs.svelte';
  import { createInstalledData, type Row } from './installed-data.svelte';
  import { createInstalledFilters } from './installed-filters.svelte';
  import { createUpdateCheck } from './update-check.svelte';
  import { createDepGraph, type RequiredByEntry } from './dep-graph.svelte';
  import {
    createPreflight,
    hasBlocking,
    installMissing,
    planVersionFix,
    remediatePickedVersion,
    violationKey,
  } from '$lib/mods/preflight.svelte';
  import { depDisplayName, type PlanSide, type PlanState } from '$lib/mods/violation-view';
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
  import {
    aliasesFor,
    crossIdsGeneration,
    findInstalled,
    learnCrossIds,
    loadAliases,
  } from '$lib/mods/cross-ids.svelte';
  import { depNameOf, depProjectOf, resolveDepNames } from '$lib/mods/dep-names.svelte';
  import { type DepTreeCtx, edgeConflict, type ModTarget } from '$lib/mods/dep-node-state';
  import { countFixed, fixAll } from '$lib/mods/fix-all';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';
  import { createInstalledSelection } from './installed-selection.svelte';
  import UpdateReviewDialog from './UpdateReviewDialog.svelte';
  import { buildReviewItems, depsLines, type UpdateReviewItem } from './update-review';
  import PreflightPanel from '$lib/mods/PreflightPanel.svelte';
  import { createCompatCheck } from './compat-check.svelte';
  import { isProblem, type ModStatus, statusOf } from './mod-status';
  import { type RowFix, type RowProblem, rowProblemOf } from './row-problem';
  import { relationFigures, relationInput, relationSlotDigits } from './relation-cell';
  import { modKey, rowDisplayName } from './row-utils';
  import InstalledToolbar from './InstalledToolbar.svelte';
  import BulkActionBar, { type BulkBarAction } from '$lib/ui/BulkActionBar.svelte';
  import { refocusAfterRemoval as refocusInList } from '$lib/ui/refocus-after-removal';
  import InstalledModRow from './InstalledModRow.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import { openExternalHttps } from '$lib/ui/safe-open';
  import { stickyEdge } from '$lib/ui/sticky-edge';

  let {
    instanceId,
    instanceName = null,
    mcVersion,
    loader,
    requestedFilter = null,
    onFilterApplied = () => {},
    onBrowseFor = (_q: string) => {},
    emptyDropzone,
    onEmptyChange = () => {},
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
    /** The host's full drop area, rendered in the empty list (passed only while it shows). */
    emptyDropzone?: Snippet;
    /** Loaded and empty — the host hides its strip meanwhile (DESIGN.md §14). */
    onEmptyChange?: (empty: boolean) => void;
  } = $props();

  // --- composables (creation order matters; thunks keep cross-refs lazy) ---
  const data = createInstalledData(() => instanceId);
  // Empty is reported, never assumed: no profile, a list still loading or a read that failed is
  // not an empty list (fallback Q2) — the host keeps its strip until the list says it is empty.
  const listEmpty = $derived(
    instanceId !== null && !data.loading && data.rows.length === 0 && data.error === null,
  );
  $effect(() => {
    onEmptyChange(listEmpty);
    return () => onEmptyChange(false);
  });
  // `opScope` is a hoisted function; it only runs once an update does.
  const updates = createUpdateCheck(() => instanceId, data.refresh, opScope);
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
  // warning. A hold never changes a problem level; it only hides an update.
  const statusBySha = $derived(
    new Map<string, ModStatus>(
      data.rows.map((r) => [
        r.installed.sha1,
        statusOf({
          enabled: r.installed.enabled,
          violations: violationsBySha.get(r.installed.sha1) ?? [],
          compat: compatHintOf(r.installed.sha1),
          update: updates.updateChecks.get(r.installed.sha1)?.state ?? null,
          held: updates.isHeld(r.installed),
        }),
      ]),
    ),
  );
  // The chip turns red only while a row IS red — the same statuses it counts.
  const anyBlocking = $derived([...statusBySha.values()].some((s) => s.level === 'blocking'));
  // The backend roots the dependency graph at the ENABLED mods only, and after a
  // toggle the list is re-read before the re-resolved graph lands — so for that
  // moment the graph is stale. What the rows can tell is taken from the rows:
  // - a mod switched off since requires nothing at load time: only roots that
  //   are enabled NOW count (`rowRequiredBy`);
  // - a mod switched on since has no root yet, so what it requires is unknown:
  //   the graph views are no fact until the graph knows every enabled platform
  //   mod (`graphCoversEnabled`) — a library it needs could read as unused;
  // - a root whose installed version the platform could not describe (offline,
  //   rate-limited, an unidentified version) requires something unknown: while
  //   one is enabled no library is unused (`enabledDepsUnknown`). «Нужны другим»
  //   stays — every edge it counts is real, so its count is a lower bound.
  // Who a row's jar serves: the mods enabled now that require its project — and
  // nobody while the jar itself is switched off. `deps.requiredBy` is keyed by
  // PROJECT, so a switched-off copy next to an enabled one read the enabled
  // copy's dependents (2026-10-02 regression F03); a switched-off jar satisfies
  // nobody, the same rule as the filters below. One rule for the row's figure
  // and section, the column's width, and the filters.
  const rowRequiredBy = (r: Row | undefined): RequiredByEntry[] =>
    r?.installed.enabled
      ? (deps.requiredBy.get(r.installed.project_id ?? '') ?? []).filter(
          (e) => rowBySha.get(e.sha1)?.installed.enabled === true,
        )
      : [];
  const requiredByCount = (r: Row | undefined): number => rowRequiredBy(r).length;
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
      // `library === true` only: `null` (a source that cannot tell) is never a library. And
      // "required by nothing" only while every enabled mod's requirements are known.
      isUnusedLibrary: (id) => {
        const r = rowBySha.get(id);
        return (
          !!r &&
          r.installed.enabled &&
          r.summary?.library === true &&
          !enabledDepsUnknown &&
          requiredByCount(r) === 0
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

  // Each installed jar's id on the other platform (spec 2026-10-08): Parasites installed from
  // Modrinth is the CurseForge project its addons from there name. Learned after every list load
  // — the backend asks nothing when nothing is new — through the one pass per profile every view
  // shares (`$lib/mods/cross-ids.svelte`). What it learns reaches this view as the profile's
  // generation (the page moves it on the backend's word): the graph reads it in `dep-graph`, the
  // alias map below. The aliases also follow the rows: an alias belongs to a current row.
  $effect(() => {
    const id = instanceId;
    // Every list load: the first, and each refresh after a mod change.
    void data.rows;
    if (id === null || data.loading) return;
    untrack(() => {
      void learnCrossIds(id);
      void loadAliases(id);
    });
  });
  $effect(() => {
    const id = instanceId;
    if (id === null) return;
    void crossIdsGeneration(id);
    untrack(() => void loadAliases(id));
  });
  const aliases = $derived(
    instanceId === null ? new Map<string, string>() : aliasesFor(instanceId),
  );
  // The relation column's slot widths, in figures (DESIGN.md §9): the longest figure of each slot
  // over every row of the profile — not the page, not the filter, so neither moves a name — from
  // the inputs each row gets (the `{#each}` below).
  const relationDigits = $derived(
    relationSlotDigits(
      data.rows.map((row) => {
        const root = deps.rootBySha.get(row.installed.sha1);
        return relationFigures(
          relationInput(root, deps.depCounts(root).total, requiredByCount(row)),
        );
      }),
    ),
  );
  // An enabled mod whose dependencies the graph could not learn (see above); a root switched
  // off since requires nothing at load time.
  const enabledDepsUnknown = $derived(
    (deps.graph?.roots ?? []).some(
      (r) => !!r.deps_unknown && rowBySha.get(r.sha1)?.installed.enabled === true,
    ),
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
  // spinner on the row; `deadEnd` flips it to the no-fix affordances (open mod
  // page / find alternative); `plans` holds the version planner's answer for a
  // conflict (spec §6.5). The picker + find-alternative dialogs are driven by
  // the *Violation holders below.
  let preflightBusy = $state(new SvelteSet<string>());
  let preflightDeadEnd = $state(new SvelteSet<string>());
  const plans = new SvelteMap<string, PlanState>();
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
  // A dependency's display name, by the rule every surface shares (`depDisplayName`): the
  // provider's own name as the report gives it — the disabled jar to switch back on, or the
  // enabled one a range points at — else the dependent-scoped store (spec §5.3), else the raw
  // loader id. The Play gate names the same report the same way (plan §5b V1).
  function depName(v: DepViolation): string {
    return depDisplayName(v, depNameOf(instanceId, v.dependent_sha1, v.dep_id));
  }

  // What the dependency trees need to say what the loader does (spec §6.3). The FULL report, not
  // the blocking subset: "without it the game won't start" stays true while a pack is still
  // completing itself. A tree knows a dependency by its project only, so its «Enable» looks the
  // disabled jar up by (source, project id) and takes the row's own guarded path. A version
  // mismatch belongs to the node's own dependent (`edgeConflict`, per edge — never another mod's
  // range on the same project), and its «Fix…» asks about that conflict — only a BLOCKING one:
  // the planner's offers show in the «What stops the game» row, which lists nothing else.
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
      // Switched on or removed since the graph was built: nothing is off to switch on, and
      // that change is already re-resolving the graph.
      if (!sha1) return;
      void setEnabled([{ sha1, name: nameBySha.get(sha1) ?? node.name }], true);
    },
    // The row's own Disable and Remove, for the jar the node stands for (spec 2026-10-07 D3a).
    // The tree acts only on a node whose install is done (`installing`), so a missing jar here
    // means the graph has not caught up with a change that is already re-resolving it.
    // A «Required by» row names its very jar (`sha1`); a tree node is looked up by project.
    onDisable: (target) => {
      const sha1 = target.sha1 ?? enabledShaByKey.get(`${target.source}:${target.project_id}`);
      if (!sha1) return;
      const refocus = panelRefocus();
      const index = pagedIndexOf((r) => r.installed.sha1 === sha1);
      void setEnabled([{ sha1, name: nameBySha.get(sha1) ?? target.name }], false).then(() =>
        refocus(index),
      );
    },
    onUninstall: (target) => {
      const sha1 = target.sha1 ?? jarOf(target);
      const row = sha1 ? rowBySha.get(sha1) : undefined;
      if (!row) return;
      void uninstall(row, panelRefocus());
    },
    installing: (key) => deps.isInstalling(key),
    conflictOf: (node, dependentSha1) => edgeConflict(blockingViolations, node, dependentSha1),
    onPlan: (v) => fixVersion(v),
  };
  // The jar a tree node or a «Required by» entry stands for: an installed node's enabled jar, a
  // switched-off one's disabled jar — the maps the tree's Enable already reads.
  function jarOf(node: ModTarget): string | null {
    const key = `${node.source}:${node.project_id}`;
    if (node.installed === false) return disabledShaByKey.get(key) ?? null;
    return enabledShaByKey.get(key) ?? disabledShaByKey.get(key) ?? null;
  }

  // Reset per-row remediation state on instance switch. The keys are dep-based
  // (dependent_sha1:dep_id), not instance-scoped, so a stale busy spinner or
  // dead-end could otherwise bleed onto a same-named dep in another instance.
  $effect(() => {
    void instanceId;
    preflightBusy.clear();
    preflightDeadEnd.clear();
    plans.clear();
    pickerViolation = null;
    findAltViolation = null;
    offPlatformPrompt = null;
    updateReview = null;
  });

  // A plan answers for the mods it read, and so does a dead end. Once the
  // pre-flight reads the folder again — any install, toggle or removal, here or
  // elsewhere — neither may still hold: a switch that "breaks nothing" may break
  // a mod added since (D8). So both go, and the row's «Fix…» asks again.
  $effect(() => {
    void preflight.report;
    plans.clear();
    preflightDeadEnd.clear();
  });

  async function refreshAfterRemediate(): Promise<void> {
    preflight.invalidate();
    deps.invalidateGraph();
    await data.refresh();
  }

  // «Fix…» on a version conflict (spec §6.5) — from the panel row, the row's
  // own line or the dependency tree: ask the two-sided planner, the network
  // only on this click. Its offers show in the panel row. An answer is not
  // asked for again until the mods change (the effect above clears it).
  async function planFix(v: DepViolation): Promise<void> {
    const id = instanceId;
    const key = violationKey(v);
    if (!id) return;
    const from = typeof document === 'undefined' ? null : document.activeElement;
    const asked = plans.get(key)?.status;
    // Answered already — offers, or the dead end: take the user there.
    if (asked === 'ready' || preflightDeadEnd.has(key)) return handFocusToPanelRow(key, from);
    // One look at a time, and none while a fix of this row is being applied.
    if (asked === 'loading' || preflightBusy.has(key)) return;
    const report = preflight.report;
    plans.set(key, { status: 'loading' });
    const answer = await planVersionFix(id, v);
    // Another profile now, or mods that changed while it was being made (the
    // effect above has already dropped its spinner): the row asks again.
    if (instanceId !== id || preflight.report !== report) return;
    if (answer.status === 'dead_end') {
      plans.delete(key);
      preflightDeadEnd.add(key);
    } else {
      plans.set(key, answer);
    }
    await handFocusToPanelRow(key, from);
  }

  // «Fix…» away from the panel — the mod's own line, the dependency tree: the
  // offers show in the panel row, so ask, and bring that row into view.
  function fixVersion(v: DepViolation): void {
    void planFix(v);
    void revealInPanel(v);
  }

  // Apply the side the user clicked, through the existing switch flows: the new
  // build downloads before the old jar goes, an off-platform build is asked
  // about, a toast says what happened, and the pre-flight runs again — only it
  // says whether the row is gone.
  async function applyPlan(v: DepViolation, side: PlanSide): Promise<void> {
    const id = instanceId;
    const key = violationKey(v);
    const st = plans.get(key);
    if (!id || st?.status !== 'ready' || preflightBusy.has(key)) return;
    const { update_dependent: dependent, change_provider: provider } = st.plan;
    preflightBusy.add(key);
    try {
      if (side === 'dependent' && dependent) {
        const name = nameBySha.get(v.dependent_sha1) ?? v.dependent_name;
        await runVersionInstall(id, name, v.dependent_sha1, dependent.version, {});
      } else if (side === 'provider' && provider) {
        await runPickedInstall(id, v, provider.version, {});
      }
    } finally {
      // Spent either way: a switch changed the mods, and a failed one may have.
      plans.delete(key);
      preflightBusy.delete(key);
    }
  }

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
    // preflight has no `provider_sha1` for it — also under the project's id on
    // the other platform (spec 2026-10-08 aliases-everywhere): the pick is then
    // a version switch, and a switch never installs beside the old jar.
    const existing =
      findInstalled(data.rows, aliases, chosen.source, chosen.project_id)?.installed ?? null;
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
    pushWarning(get(t)('mods.browse.toastInstallFailed'), r.error ? [modWriteReason(r.error)] : []);
  }

  // One-click install of a missing required dependency from the pre-flight
  // panel or the row. Resolves the dep by its loader mod-id and installs it; on
  // success the panel + graph refresh. A dep the backend cannot resolve with
  // confidence (`open_search`) — or an install a manual pick may get past —
  // hands the query up to the Add-ons shell, which switches to Browse with the
  // search pre-filled. Two failures are no miss, and a search would lie about
  // them: a project the profile already lists (the search invites the second
  // copy that stops the game — say so and re-read the report instead), and a
  // busy profile (nothing can install until it is free).
  const onInstallMissingDep = async (v: DepViolation): Promise<void> => {
    if (!instanceId) return;
    const outcome = await installMissing(instanceId, v.dependent_sha1, v.dep_id);
    if (outcome.kind === 'installed') {
      // Its own required dependencies come along: said, never installed silently (D9).
      const tt = get(t);
      pushSuccess(
        tt('mods.browse.toastInstalledMod', { name: outcome.name }),
        depsLines(tt, [outcome.summary]),
      );
      await refreshAfterRemediate();
      return;
    }
    if (outcome.kind === 'failed' && outcome.error.kind === 'mods_already_installed') {
      pushInfo(formatError(outcome.error));
      await refreshAfterRemediate();
      return;
    }
    if (outcome.kind === 'failed' && outcome.error.kind === 'instance_busy') {
      pushWarning(get(t)('mods.browse.toastInstallFailedWithMod', { name: depName(v) }), [
        modWriteReason(outcome.error),
      ]);
      return;
    }
    const why = outcome.kind === 'failed' ? [modWriteReason(outcome.error)] : [];
    pushWarning(get(t)('mods.preflight.installSearchFallback', { dep: depName(v) }), why);
    onBrowseFor(outcome.kind === 'open_search' ? outcome.query : v.dep_id);
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
  // modToggle event re-runs the pre-flight. `depName` is the provider's own name, as the
  // report gives it.
  function enableProvider(v: DepViolation): Promise<void> {
    const sha1 = v.provider_sha1;
    if (!sha1) return Promise.resolve();
    return setEnabled([{ sha1, name: depName(v) }], true);
  }

  // A «What stops the game» row by its key — matched by value, not by a
  // selector: a key is data (a sha1 and a mod id).
  const panelRow = (key: string): HTMLElement | undefined =>
    [...document.querySelectorAll<HTMLElement>('[data-violation-key]')].find(
      (el) => el.dataset.violationKey === key,
    );

  // Bring a violation's panel row into view: the row line's «and N more», and
  // its «Fix…» — the planner's offers show in that row.
  async function revealInPanel(v: DepViolation): Promise<void> {
    await tick();
    if (typeof document === 'undefined') return;
    panelRow(violationKey(v))?.scrollIntoView?.({ behavior: 'smooth', block: 'center' });
  }

  // The planner's answer lands in the panel row; focus follows it when the user
  // is still where they asked from (the row's or the tree's «Fix…») or lost it
  // with the panel's own button, which the spinner replaced — never pulled from
  // wherever they went meanwhile.
  async function handFocusToPanelRow(key: string, from: Element | null): Promise<void> {
    await tick();
    if (typeof document === 'undefined') return;
    const active = document.activeElement;
    if (active !== from && active !== null && active !== document.body) return;
    panelRow(key)?.querySelector<HTMLElement>('button')?.focus();
  }

  // «Show in the list» from a panel row (and, by project, from the trees). A search or a chip that hides the dependent is cleared
  // for the view that holds it: the problem view while it is a problem (it is,
  // while the report is current), else every mod. A report that predates a
  // removal names a mod no view holds — say so, and change no filter for
  // nothing (clearing them would show nothing anyway).
  function jumpToDependent(v: DepViolation): Promise<void> {
    return jumpToJar(v.dependent_sha1, v.dependent_name);
  }
  // The row a «show in the list» landed on, until it has flashed (spec 2026-10-07 §9): the row's
  // `fieldFlash` fires on the edge and reports delivery, which clears it — one flash per click.
  let flashSha = $state<string | null>(null);
  async function jumpToJar(sha1: string, name: string): Promise<void> {
    if (await deps.jumpToSha1(sha1)) {
      flashSha = sha1;
      return;
    }
    const gone = () => pushInfo(get(t)('mods.preflight.dependentGone', { name }));
    if (!rowBySha.has(sha1)) {
      gone();
      return;
    }
    filters.filter = '';
    filters.viewFilter = isProblem(statusBySha.get(sha1)) ? 'issues' : 'all';
    await tick();
    // Removed in the meantime (the list re-read while the view changed).
    if (await deps.jumpToSha1(sha1)) flashSha = sha1;
    else gone();
  }
  // The tree's and «Required by»'s locate: the same way, once the jar is looked up by project —
  // never a silent nothing when a search hides the row (spec 2026-10-07 D4a).
  function jumpToProject(target: ModTarget): Promise<void> {
    // A «Required by» entry knows its very jar; a tree node is looked up by project.
    const sha1 = target.sha1 ?? jarOf(target);
    if (!sha1) {
      pushInfo(get(t)('mods.preflight.dependentGone', { name: target.name }));
      return Promise.resolve();
    }
    return jumpToJar(sha1, nameBySha.get(sha1) ?? target.name);
  }

  // «Fix all (N)»: the Play gate's repair (fix-all.ts), then a FRESH pre-flight
  // — only it says which rows are gone — and one toast «Fixed N of M», with why
  // the steps that failed failed when rows are left. The profile it ran for is
  // captured and named when the user has moved on.
  let fixAllBusy = $state(false);
  async function runFixAll(): Promise<void> {
    const id = instanceId;
    const name = instanceName;
    const report = preflight.report;
    if (!id || !report || fixAllBusy) return;
    fixAllBusy = true;
    try {
      const { attempted, reasons } = await fixAll(id, report);
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
      else pushWarning(title, [...reasons, ...where]);
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

  // «Choose version» on a mod that needs another build of ITSELF (a platform mismatch, a compat
  // warning): its own version list, in the detail modal — or null when it has no platform
  // identity to list builds from (a manual jar). One answer for the row's line and its «What
  // stops the game» row, so the two always offer the same.
  function ownVersionOpener(m: Row['installed']): (() => void) | null {
    const { source, project_id: projectId } = m;
    if (!source || !projectId) return null;
    return () => openDetailMod(source, projectId);
  }
  const ownVersionOfDependent = (v: DepViolation): (() => void) | null => {
    const row = rowBySha.get(v.dependent_sha1);
    return row ? ownVersionOpener(row.installed) : null;
  };

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
      canChooseVersion: ownVersionOpener(row.installed) !== null,
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
        fixVersion(fix.violation);
        return;
      case 'choose_version':
        // Another build of THIS mod: its own versions, in the detail modal.
        ownVersionOpener(row.installed)?.();
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
  const error = $derived(data.error ?? deps.error ?? updates.error);

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

  // The review before «Update all» (spec D9, §6.6): every pending update, all
  // ticked. Bound to the profile it opened for — a profile switch closes it.
  let updateReview = $state<UpdateReviewItem[] | null>(null);
  function openUpdateReview() {
    const items = buildReviewItems(data.rows, updates.updateChecks);
    if (items.length > 0) updateReview = items;
  }
  async function runUpdateReview(sha1s: string[]) {
    await updates.updateSelected(sha1s);
    updateReview = null;
  }

  // The installed build of the project the detail modal shows: its version id
  // AND its bytes. `enrich` leaves `version_id` null for exactly the mods whose
  // platform tags disagree with the instance, and those are recognised by sha1.
  // Its own id or its id on the other platform (spec 2026-10-08 aliases-everywhere).
  const detailInstalled = $derived.by(() => {
    if (!detail) return null;
    return findInstalled(data.rows, aliases, detail.source, detail.projectId)?.installed ?? null;
  });

  async function installDetailVersion(v: ModVersion, opts: InstallOpts = {}) {
    if (!instanceId || !detail) return;
    const existing = findInstalled(data.rows, aliases, detail.source, detail.projectId);
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
      // A refused switch says the profile is busy — never that the game runs (plan A9).
      pushWarning(get(t)('mods.browse.toastInstallFailed'), [modWriteReason(res.error)]);
    } else {
      // A switch is an update: what the new build brought in is said, never silent (D9).
      const tt = get(t);
      pushSuccess(tt('mods.browse.toastInstalledMod', { name }), depsLines(tt, [res.data]));
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
  async function uninstall(
    row: Row,
    refocus: (index: number) => Promise<void> = refocusAfterRemoval,
  ) {
    if (!instanceId) return;
    const target = [{ sha1: row.installed.sha1, name: rowDisplayName(row) }];
    const index = pagedIndexOf((r) => r.installed.sha1 === row.installed.sha1);
    data.error = null;
    shellBusy = true;
    try {
      if ((await uninstallMods(opScope(instanceId), target)) !== 'cancelled') {
        await data.refresh();
        deps.reloadGraph();
        preflight.invalidate();
        await refocus(index);
      }
    } finally {
      shellBusy = false;
    }
  }
  // Where focus goes after a Disable or Remove from a dependency panel (DESIGN.md §13), read from
  // the control pressed, before the action: nowhere while it is still on the page (a tree button
  // keeps its element whatever the node becomes) or when nothing had focus (WebKit focuses no
  // button on a click); to the «Required by» row now in its place — the next, else the one
  // before — when the row left with the dependent it named; and by the list's rule only when the
  // panel itself went with its own mod.
  function panelRefocus(): (index: number) => Promise<void> {
    const had = document.activeElement;
    const panel = had?.closest<HTMLElement>('[data-dep-section]') ?? null;
    const byRows = (el: HTMLElement) => [
      ...el.querySelectorAll<HTMLElement>('[data-required-by-row]'),
    ];
    const byRow = had?.closest<HTMLElement>('[data-required-by-row]') ?? null;
    const rowAt = panel && byRow ? byRows(panel).indexOf(byRow) : -1;
    return async (index) => {
      await tick();
      if (had === null || had === document.body || had.isConnected) return;
      if (panel?.isConnected && rowAt >= 0) {
        await refocusInList({ index: rowAt, listEl: () => panel, rows: byRows });
        return;
      }
      await refocusAfterRemoval(index);
    };
  }
  // The bulk bar's Remove: the removed rows' place is where the first of them was.
  async function bulkUninstall(): Promise<void> {
    const index = pagedIndexOf((r) => selection.selected.has(r.installed.sha1));
    await selection.requestBulkUninstall();
    await refocusAfterRemoval(index);
  }

  // After a removal the control that had focus is gone with its row — the row's Remove, a dialog
  // that returned focus to it, the bulk bar's Remove that left with the selection — and focus fell
  // to <body> (plan §5b V2). It goes to the row now in that place: the next one, else the one
  // before it, else the list — never pulled from wherever the user went meanwhile, and not at all
  // after a cancel (focus came back to the control that asked).
  let listEl = $state<HTMLElement | null>(null);
  let emptyListEl = $state<HTMLElement | null>(null);
  const pagedIndexOf = (hit: (r: Row) => boolean): number =>
    Math.max(0, filters.paged.findIndex(hit));
  async function refocusAfterRemoval(index: number): Promise<void> {
    await refocusInList({
      index,
      listEl: () => listEl,
      rows: (list) => [...list.querySelectorAll<HTMLElement>('[data-mod-row]')],
      emptyEl: () => emptyListEl,
    });
  }

  // «Перепроверить совместимость и зависимости» (⋯): the live compat check (it forces the
  // offline scan first), a fresh graph and a fresh pre-flight. A failed graph load lands in the
  // error banner (`deps.error`), a failed pre-flight in its panel; the compat check reports to no
  // surface of its own, so its failure is said here — the spinner going away must not read as
  // "all clear".
  const rechecking = $derived(compat.checking || deps.graphLoading || preflight.loading);
  async function recheckAll() {
    const id = instanceId;
    deps.invalidateGraph();
    preflight.invalidate();
    await compat.runLiveCheck();
    // A check superseded by a profile switch sets no error, and this one is no longer shown.
    if (compat.error && instanceId === id)
      pushWarning(get(t)('mods.installed.recheckFailed'), [compat.error]);
  }

  // «Открыть папку модов» (⋯) — the Overview's control, for this profile.
  async function openModsFolder() {
    if (!instanceId) return;
    const r = await commands.openModsFolder(instanceId);
    if (r.status === 'error')
      pushWarning(get(t)('instance.manage.openModsFolderFailed'), [formatError(r.error)]);
  }

  // The row menu (spec §6.8). «Показать в папке»: the jar as it is on disk (`.disabled` too),
  // selected in the file manager — also the keyboard path to the file name the version's tooltip
  // shows on hover. A failure says why; it never fails silently.
  async function revealFile(m: Row['installed']) {
    const id = instanceId;
    if (!id) return;
    let reason: string | null = null;
    try {
      const r = await commands.modsRevealFile(id, m.sha1);
      if (r.status === 'error') reason = formatError(r.error);
    } catch (e) {
      reason = e instanceof Error ? e.message : String(e);
    }
    if (reason !== null) pushWarning(get(t)('mods.card.revealFailed'), [reason]);
  }
  // «Открыть страницу мода»: only where the page is known — a CurseForge page needs its slug,
  // Modrinth takes the project id too. No slug, no page: never a guessed URL (pack-managed
  // sources have no mod page of their own).
  function projectPageOpener(row: Row): (() => void) | null {
    const { source, project_id: projectId } = row.installed;
    if (source !== 'modrinth' && source !== 'curseforge') return null;
    const slugOrId = row.summary?.slug ?? (source === 'modrinth' ? projectId : null);
    if (!slugOrId) return null;
    return () => void openExternalHttps(modProjectUrl(source, slugOrId));
  }
  // A modpack's own mods are never checked for updates — the pack owns their versions.
  const isPackMod = (row: Row): boolean =>
    !!data.packSummary && data.packSummary.mod_shas.includes(row.installed.sha1);
  // «Не обновлять» / «Разрешить обновления»: a hold is per project, so a hand-dropped jar has none
  // (nothing to update from), and a hold state that could not be read offers no control (T26:
  // "not read" is not "not held"). On a modpack's own mod a new hold would promise nothing; one
  // already set can still be released. `setHold` says itself why a change was refused.
  function holdControl(row: Row): { held: boolean; onToggle: () => void } | null {
    const m = row.installed;
    if (!m.source || !m.project_id || updates.holds === null) return null;
    const held = updates.isHeld(m);
    if (!held && isPackMod(row)) return null;
    return { held, onToggle: () => void updates.setHold(m, !held, rowDisplayName(row)) };
  }

  // Bulk update: run it, then drop the checks of the jars it replaced — in the
  // profile it ran for — so their badges don't linger (the selection composable
  // runs the updates; the checks are the persisted check's).
  async function bulkUpdate() {
    const id = instanceId;
    const updated = await selection.bulkUpdate();
    updates.forget(updated, id);
  }

  // The bulk bar's actions (DESIGN.md §9). Update is off until a selected mod has a pending
  // update; the rest stay on, as before.
  const bulkActions = $derived<BulkBarAction[]>([
    { id: 'enable', label: $t('mods.card.enable') },
    { id: 'disable', label: $t('mods.card.disable') },
    {
      id: 'update',
      label: $t('mods.card.update'),
      disabled: selection.selectedUpdatable.length === 0,
      disabledReason: $t('mods.installed.bulkUpdateTitle'),
    },
    { id: 'uninstall', label: $t('mods.card.uninstall'), intent: 'danger' },
  ]);
  function onBulkAction(id: string): void {
    if (id === 'enable') void selection.bulkSetEnabled(true);
    else if (id === 'disable') void selection.bulkSetEnabled(false);
    else if (id === 'update') void bulkUpdate();
    else if (id === 'uninstall') void bulkUninstall();
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
  //
  // A toggle refreshes exactly what an install or a removal does, the graph
  // included: it is rooted at the ENABLED mods and marks the disabled ones, so a
  // switch changes both — without a re-resolve, disabling a mod with its dependents left the tree
  // offering to enable mods that were on and the library chips counting roots
  // that were off.
  const debouncedModsChanged = debounceTrailing(() => {
    void data.refresh();
    deps.reloadGraph();
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
    events.modInstalled.listen(debouncedModsChanged.call),
    events.modUninstalled.listen(debouncedModsChanged.call),
    events.modToggle.listen(debouncedModsChanged.call),
    events.modsReconciled.listen(debouncedExternalChange.call),
  ]);
  onDestroy(() => {
    debouncedModsChanged.cancel();
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
    updateCount={updates.updateCount}
    checkedAtMs={updates.checkedAtMs}
    {rechecking}
    onCheckUpdates={updates.checkUpdates}
    onUpdateAll={openUpdateReview}
    onRecheckAll={() => void recheckAll()}
    onOpenModsFolder={() => void openModsFolder()}
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
    onInstallMissing={onInstallMissingDep}
    onEnableProvider={enableProvider}
    onJumpToDependent={jumpToDependent}
    onFixAll={runFixAll}
    {fixAllBusy}
    onChooseVersion={onPreflightChooseVersion}
    ownVersionOpener={ownVersionOfDependent}
    onFindAlternative={onPreflightFindAlternative}
    onOpenModPage={onPreflightOpenModPage}
    onMigrate={() => (migrationDialogOpen = true)}
    migrateCount={compat.incompatibleCount}
    busyKeys={preflightBusy}
    deadEndKeys={preflightDeadEnd}
    {plans}
    onPlan={planFix}
    onApplyPlan={applyPlan}
  />

  {#if !instanceId}
    <div class="text-placeholder text-sm py-8 text-center">
      {$t('mods.installed.pickInstanceFirst')}
    </div>
  {:else if data.loading && data.rows.length === 0}
    <LoadingPanel label={$t('mods.installed.loading')} />
  {:else if listEmpty}
    <!-- The host's full drop area replaces its strip here (DESIGN.md §14). A list that could not
         be read shows its error above, never «no mods». Focus lands here after the last removal
         (`refocusAfterRemoval`): a parking place that reads the message, not a control. -->
    <div
      bind:this={emptyListEl}
      tabindex="-1"
      class="pt-6 flex flex-col gap-3 outline-none"
      data-testid="list-empty"
    >
      <p class="text-placeholder text-sm text-center">{$t('mods.installed.empty')}</p>
      {@render emptyDropzone?.()}
    </div>
  {:else if data.rows.length > 0}
    <div
      bind:this={listEl}
      class="border border-border-subtle rounded overflow-hidden"
      data-testid="installed-list"
      style:--rel-dep-ch={relationDigits.dep}
      style:--rel-by-ch={relationDigits.by}
    >
      <BulkActionBar
        allSelected={selection.allSelected}
        selectedCount={selection.selected.size}
        indeterminate={selection.selected.size > 0 && !selection.allSelected}
        {busy}
        busyAction={selection.busyAction}
        hint={$t('mods.installed.bulkHint')}
        actions={bulkActions}
        onToggleAll={selection.toggleSelectAll}
        onAction={onBulkAction}
        onClear={selection.clear}
      />
      {#each filters.paged as row (row.installed.sha1)}
        {@const rowKey = modKey(row.installed.source, row.installed.project_id, row.installed.sha1)}
        {@const root = deps.rootBySha.get(row.installed.sha1)}
        {@const counts = deps.depCounts(root)}
        {@const reqBy = rowRequiredBy(row)}
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
          updateState={updates.updateChecks.get(row.installed.sha1)?.state ?? null}
          held={updates.isHeld(row.installed)}
          checking={updates.checking}
          packChip={data.packSummary && data.packSummary.mod_shas.includes(row.installed.sha1)
            ? data.packSummary.project_name
            : null}
          selected={selection.selected.has(row.installed.sha1)}
          {treeCtx}
          onToggleExpand={() => deps.toggleExpand(row.installed.sha1)}
          onOpenDetail={() => {
            if (row.installed.source && row.installed.project_id)
              openDetailMod(row.installed.source as ModSource, row.installed.project_id);
          }}
          onOpenDetailMod={openDetailMod}
          onToggle={() => toggle(row)}
          onUninstall={() => uninstall(row)}
          onUpdate={() => updates.updateOne(row.installed, rowDisplayName(row))}
          onShowChangelog={() => openChangelog(row)}
          onSelectChange={(c) => selection.toggleSelect(row.installed.sha1, c)}
          onInstallDep={deps.installDepNode}
          onJump={jumpToProject}
          onProblemFix={(fix) => onRowFix(row, fix)}
          onRevealProblems={() => {
            const first = violationsBySha.get(row.installed.sha1)?.[0];
            if (first) void revealInPanel(first);
          }}
          onRevealFile={() => void revealFile(row.installed)}
          onOpenProjectPage={projectPageOpener(row)}
          hold={holdControl(row)}
          flash={flashSha === row.installed.sha1}
          onFlashed={() => (flashSha = null)}
        />
      {/each}
    </div>

    <!-- Pagination footer — unified with Browse/Modpacks (Steam-style). One page: no pager
         (spec §6.7). The size picker stays while a smaller page would still split the list, or
         picking 100 would strand the user without it (plan A20 / P5-3). -->
    {#if filters.pageCount > 1}
      <div
        class="sticky bottom-0 z-10 bg-base border-t border-border-subtle"
        use:stickyEdge={'bottom'}
      >
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
    {:else if filters.filtered.length > PAGE_SIZES[0]}
      <div class="flex justify-end pt-2">
        <PageSizePicker prefsKey="installedPageSize" />
      </div>
    {/if}
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

  {#if updateReview}
    <UpdateReviewDialog
      items={updateReview}
      busy={updates.busy}
      onCancel={() => (updateReview = null)}
      onConfirm={(shas) => void runUpdateReview(shas)}
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
