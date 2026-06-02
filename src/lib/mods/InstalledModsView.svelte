<script lang="ts">
  import {
    commands,
    events,
    type DepRoot,
    type DepTreeNode,
    type DependencyGraph,
    type InstalledMod,
    type LoaderKind,
    type ModSource,
    type ModSummary,
    type ModUpdateCheck,
    type ModVersion,
    type PackOriginSummary,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { settingsOpen } from '$lib/settings/state.svelte';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { get } from 'svelte/store';
  import { onDestroy, onMount, tick } from 'svelte';
  import { PAGE_SIZES } from './browser-prefs.svelte';
  import CurseForgeKeyBanner from './CurseForgeKeyBanner.svelte';
  import DepTree from './DepTree.svelte';
  import ModCard from './ModCard.svelte';
  import ModDetailModal from './ModDetailModal.svelte';
  import OrphanUninstallDialog from './OrphanUninstallDialog.svelte';
  import { mapLimit } from './concurrency';
  import { depGraphCache } from './dep-graph-cache';
  import { updateCheckCache } from './update-check-cache';
  import type { OrphanRef } from '$lib/ipc/bindings';

  // The Installed pane of ModBrowserTab. Renders the same ModCard
  // component the Browse pane uses, so the UI is consistent — same
  // icons, layout, Disable/Enable + Uninstall affordances. The only
  // difference is the list is filtered to mods currently installed in
  // the active instance.
  //
  // Each row pairs a ModSummary (fetched lazily from the platform per
  // installed mod's project_id) with the InstalledMod record from
  // {instance}/lucerna/installed-mods.json. Manual mods (jars the
  // user dropped into the mods folder by hand — source: null) render a
  // degraded row with no icon and the filename as the title, since
  // they have no platform metadata to fetch.
  //
  // Event listeners are belt-and-suspenders: the action handlers also
  // call refresh() directly after each IPC since the typed
  // events.modX.listen channels don't always match the backend's
  // string-emit channel names.

  let {
    instanceId,
    mcVersion,
    loader,
  }: {
    instanceId: string | null;
    mcVersion: string | null;
    loader: LoaderKind | null;
  } = $props();

  type Row = {
    summary: ModSummary | null;
    installed: InstalledMod;
  };

  let rows = $state<Row[]>([]);
  let filter = $state('');
  let enabledFilter = $state<'all' | 'enabled' | 'disabled'>('all');

  // WCAG radiogroup keyboard pattern. Arrow / Home / End moves selection
  // within the group; the newly-checked radio gets focus so a screen reader
  // announces the change. Roving tabindex (0 on checked, -1 elsewhere) keeps
  // the whole group as one tab stop.
  const FILTER_VALUES = ['all', 'enabled', 'disabled'] as const;
  function handleFilterKey(e: KeyboardEvent) {
    const i = FILTER_VALUES.indexOf(enabledFilter);
    let next: (typeof FILTER_VALUES)[number] | null = null;
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
      next = FILTER_VALUES[(i + 1) % FILTER_VALUES.length];
    } else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
      next = FILTER_VALUES[(i - 1 + FILTER_VALUES.length) % FILTER_VALUES.length];
    } else if (e.key === 'Home') {
      next = FILTER_VALUES[0];
    } else if (e.key === 'End') {
      next = FILTER_VALUES[FILTER_VALUES.length - 1];
    }
    if (next !== null) {
      e.preventDefault();
      enabledFilter = next;
      const target = e.currentTarget as HTMLElement | null;
      const btn = target?.querySelector<HTMLButtonElement>(`button[data-value="${next}"]`);
      btn?.focus();
    }
  }
  let sortBy = $state<'name-asc' | 'name-desc' | 'recent' | 'source'>('name-asc');
  let error = $state<string | null>(null);
  let loading = $state(false);
  let busy = $state(false);

  // Open the version drawer when the user clicks a card body. The
  // drawer shows the full version list with the installed version
  // highlighted; clicking another version triggers a swap (uninstall
  // current + install new) via switchVersion below.
  let drawerRow = $state<Row | null>(null);

  // Mod-update check. `updateChecks` is keyed by installed-mod sha1 and
  // holds only eligible (non-pack, platform-installed) mods. Seeded from
  // the per-instance session cache so reopening the tab is instant; the
  // "Check for updates" button forces a fresh check. `packSummary` feeds
  // the per-row "from modpack" chip and is loaded on every refresh().
  let updateChecks = $state<Map<string, ModUpdateCheck>>(new Map());
  let checking = $state(false);
  let packSummary = $state<PackOriginSummary | null>(null);
  let showCfBanner = $state(false);

  // Dependency graph — loaded in the background, never blocks the mod list.
  let graph = $state<DependencyGraph | null>(null);
  let graphLoading = $state(false);
  // Set of installed-mod sha1s whose dep subtree is currently expanded.
  // Reassign the whole Set (never mutate in place) to trigger reactivity.
  let expanded = $state<Set<string>>(new Set());
  // Hovered dep-key (`source:project_id`) for cross-highlight. Shared
  // between row wrappers and DepTree nodes so every on-screen occurrence
  // of the same mod lights up together.
  let hoveredKey = $state<string | null>(null);

  // Returns the canonical highlight key for a mod. Platform mods use
  // `source:project_id`; manual mods (source or project_id absent) fall
  // back to `sha1:<sha1>` so they only highlight themselves.
  const modKey = (source: string | null, projectId: string | null, sha1: string) =>
    source && projectId ? `${source}:${projectId}` : `sha1:${sha1}`;

  // Strategy C: render the mod list immediately; resolve the graph in the
  // background. Seed from the session cache on instance change so switching
  // back to an already-visited instance is instant.
  $effect(() => {
    const id = instanceId;
    graph = id ? (depGraphCache.get(id) ?? null) : null;
    if (id) void loadGraph(id);
  });

  async function loadGraph(id: string) {
    graphLoading = true;
    const r = await commands.modsDependencyGraph(id);
    // Guard against an instance-switch race: if the user moved to another
    // instance while this (possibly slow) resolve was in flight, drop the
    // stale result rather than rendering it under the wrong instance.
    if (instanceId !== id) return;
    graphLoading = false;
    if (r.status === 'ok') {
      graph = r.data;
      depGraphCache.set(id, r.data);
    }
  }

  // Force a fresh dependency-graph resolve. Call after anything that changes
  // the installed SET (install / uninstall) so the tree's satisfied/missing
  // state doesn't go stale. Enable/disable doesn't change the set, so those
  // paths don't need it. Debounced: a bulk uninstall emits one event per mod,
  // and in-view handlers also call this directly — without debouncing that
  // would trigger many (expensive, fan-out) resolves; here they collapse into
  // one after the burst settles.
  let graphReloadTimer: ReturnType<typeof setTimeout> | null = null;
  function reloadGraph() {
    if (!instanceId) return;
    if (graphReloadTimer) clearTimeout(graphReloadTimer);
    graphReloadTimer = setTimeout(() => {
      graphReloadTimer = null;
      if (instanceId) {
        depGraphCache.delete(instanceId);
        void loadGraph(instanceId);
      }
    }, 150);
  }

  function recheckDeps() {
    reloadGraph();
  }

  // Scroll the list row for a dependency node into view and highlight it, so
  // clicking a dep in an expanded tree jumps to that mod's own row — flipping
  // to the right page first when the target is paged out of view.
  async function jumpToMod(node: DepTreeNode) {
    const key = `${node.source}:${node.project_id}`;
    hoveredKey = key;
    const idx = filtered.findIndex(
      (r) => modKey(r.installed.source, r.installed.project_id, r.installed.sha1) === key,
    );
    if (idx < 0) return; // not in the current filter (e.g. filtered out)
    page = Math.floor(idx / pageSize);
    await tick();
    if (typeof document !== 'undefined') {
      const el = document.querySelector(`[data-mod-row="${key}"]`);
      // scrollIntoView is absent in some test DOMs — call it optionally.
      (el as HTMLElement | null)?.scrollIntoView?.({ behavior: 'smooth', block: 'center' });
    }
  }

  const rootBySha = $derived(new Map((graph?.roots ?? []).map((r) => [r.sha1, r])));

  // Build a reverse map: for each installed mod's project_id, list the
  // display names of roots that depend on it (via required subtree).
  // The graph root's own `name` is the registry name, which for many mods is
  // the version's release title (e.g. "0.26.3"). Prefer the resolved project
  // name from the loaded rows so "Required by" shows real mod names.
  const requiredBy = $derived.by(() => {
    const nameBySha = new Map(rows.map((r) => [r.installed.sha1, rowDisplayName(r)]));
    const map = new Map<string, string[]>();
    for (const r of graph?.roots ?? []) {
      const rootName = nameBySha.get(r.sha1) ?? r.name;
      const seen = new Set<string>();
      const walk = (ns: DepTreeNode[]) => {
        for (const n of ns) {
          if (n.status === 'satisfied' && !seen.has(n.project_id)) {
            seen.add(n.project_id);
            map.set(n.project_id, [...(map.get(n.project_id) ?? []), rootName]);
          }
          if (!n.cycle) walk(n.children);
        }
      };
      walk(r.required);
    }
    return map;
  });

  function depCounts(root: DepRoot | undefined) {
    if (!root) return { total: 0, missing: 0 };
    let total = 0;
    let missing = 0;
    const walk = (ns: DepTreeNode[]) => {
      for (const n of ns) {
        total++;
        if (n.status === 'missing_required') missing++;
        if (!n.cycle) walk(n.children);
      }
    };
    walk(root.required);
    return { total, missing };
  }

  function toggleExpand(sha1: string) {
    const next = new Set(expanded);
    if (next.has(sha1)) next.delete(sha1);
    else next.add(sha1);
    expanded = next;
  }

  async function installDepNode(node: DepTreeNode) {
    if (!instanceId || !mcVersion || !loader) return;
    busy = true;
    error = null;
    const vr = await commands.modsVersions(node.source, node.project_id, mcVersion, loader);
    if (vr.status === 'error' || vr.data.length === 0) {
      error =
        vr.status === 'error'
          ? formatError(vr.error)
          : get(t)('mods.installed.installDepFailed', { name: node.name });
      busy = false;
      return;
    }
    const primary = vr.data[0];
    const res = await commands.modsInstallWithDeps(
      instanceId,
      { source: primary.source, project_id: primary.project_id, version_id: primary.version_id },
      [],
    );
    if (res.status === 'error') {
      pushWarning(get(t)('mods.browse.toastInstallFailed'), [formatError(res.error)]);
    } else {
      pushSuccess(get(t)('mods.browse.toastInstalledMod', { name: node.name }));
    }
    busy = false;
    if (instanceId) {
      depGraphCache.delete(instanceId);
      await loadGraph(instanceId);
    }
    await refresh();
  }

  const updateCount = $derived(
    [...updateChecks.values()].filter((c) => c.state.kind === 'update_available').length,
  );

  function rowDisplayName(r: Row): string {
    return r.summary?.name ?? r.installed.name;
  }

  // Sort the unfiltered rows according to the user's choice. Default
  // is name-asc (predictable, and a version swap doesn't shuffle the
  // mod's slot since uninstall+install appends to installed-mods.json
  // under the hood).
  const sorted = $derived.by(() => {
    const xs = [...rows];
    const nameLower = (r: Row) => rowDisplayName(r).toLowerCase();
    switch (sortBy) {
      case 'name-asc':
        return xs.sort((a, b) => nameLower(a).localeCompare(nameLower(b)));
      case 'name-desc':
        return xs.sort((a, b) => nameLower(b).localeCompare(nameLower(a)));
      case 'recent':
        // installed_at is RFC 3339; string compare works for ISO dates,
        // newest first.
        return xs.sort((a, b) => b.installed.installed_at.localeCompare(a.installed.installed_at));
      case 'source':
        // Modrinth / CurseForge / manual (null), then alphabetic
        // within each group.
        return xs.sort((a, b) => {
          const sa = a.installed.source ?? 'zz-manual';
          const sb = b.installed.source ?? 'zz-manual';
          if (sa !== sb) return sa.localeCompare(sb);
          return nameLower(a).localeCompare(nameLower(b));
        });
    }
  });

  const filtered = $derived(
    sorted
      .filter((r) => {
        if (enabledFilter === 'enabled') return r.installed.enabled;
        if (enabledFilter === 'disabled') return !r.installed.enabled;
        return true;
      })
      .filter(
        (r) =>
          filter.trim() === '' || rowDisplayName(r).toLowerCase().includes(filter.toLowerCase()),
      ),
  );

  // Pagination. The installed list can run to a few hundred mods, so render
  // one page at a time. Selection and the dep graph still operate over the
  // whole filtered set — only what's *rendered* is paged.
  let pageSize = $state<number>(50);
  let page = $state(0);
  const pageCount = $derived(Math.max(1, Math.ceil(filtered.length / pageSize)));
  // Reset to the first page whenever the result set's shape changes.
  $effect(() => {
    void filter;
    void enabledFilter;
    void sortBy;
    void instanceId;
    void pageSize;
    page = 0;
  });
  // Keep the page in range if the list shrinks (e.g. after uninstall).
  $effect(() => {
    if (page > pageCount - 1) page = Math.max(0, pageCount - 1);
  });
  const paged = $derived(filtered.slice(page * pageSize, page * pageSize + pageSize));

  // Selection state for bulk actions. `selected` holds the sha1s of the
  // currently checked rows. Reassign the whole Set (never mutate in place)
  // to trigger Svelte 5 reactivity.
  let selected = $state<Set<string>>(new Set());

  // Drop selections for rows no longer visible (filter/search change) so a
  // hidden row can't be bulk-acted on.
  $effect(() => {
    const visible = new Set(filtered.map((r) => r.installed.sha1));
    let changed = false;
    const next = new Set(selected);
    for (const sha of next)
      if (!visible.has(sha)) {
        next.delete(sha);
        changed = true;
      }
    if (changed) selected = next;
  });
  $effect(() => {
    const id = instanceId;
    selected = new Set();
    // Drop per-instance view state too, so nothing carries across a switch.
    expanded = new Set();
    hoveredKey = null;
    // Blank the previous instance's mods and any sticky error immediately on
    // switch so stale content never lingers while the new list loads. (refresh
    // also clears error, but doing it here avoids a flash of the old list.)
    if (id) {
      rows = [];
      error = null;
    }
  });

  const allSelected = $derived(
    filtered.length > 0 && filtered.every((r) => selected.has(r.installed.sha1)),
  );
  function toggleSelect(sha1: string, checked: boolean) {
    const next = new Set(selected);
    if (checked) next.add(sha1);
    else next.delete(sha1);
    selected = next;
  }
  function toggleSelectAll(checked: boolean) {
    selected = checked ? new Set(filtered.map((r) => r.installed.sha1)) : new Set();
  }

  // Counters reflect the full installed list (pre-filter) so the user
  // sees the inventory total even when narrowing by name.
  const totalCount = $derived(rows.length);
  const enabledCount = $derived(rows.filter((r) => r.installed.enabled).length);
  const disabledCount = $derived(totalCount - enabledCount);

  async function refresh() {
    // Capture the instance this refresh is for. Several awaits happen below;
    // if the user switches instances mid-flight we must NOT commit this
    // (now stale) instance's data over the newer one — otherwise the old
    // instance's mod list can overwrite the current view, and acting on that
    // stale list (bulk uninstall, etc.) is dangerous. Every commit is guarded
    // by `instanceId === reqId`.
    const reqId = instanceId;
    if (!reqId) {
      rows = [];
      return;
    }
    loading = true;
    error = null;
    let r = await commands.modsListInstalled(reqId);
    if (instanceId !== reqId) return;
    if (r.status === 'error') {
      error = formatError(r.error);
      loading = false;
      return;
    }

    // Pack-origin chip data — a local file read, no network.
    const ps = await commands.modsPackOriginSummary(reqId);
    if (instanceId !== reqId) return;
    const summary = ps.status === 'ok' ? ps.data : null;
    packSummary = summary;

    // Backfill: if this instance has modpack override-bundled mods that
    // still lack a platform identity and have never been hash-enriched,
    // run one enrichment pass and re-fetch the list. `enrich_attempted`
    // flips true only when EVERY platform the pass tried responded
    // successfully — a transient CF/Modrinth outage leaves the flag
    // unflipped, so this branch can fire again on a later Installed-tab
    // open until both platforms answered. Bounded by tab events, not a
    // hot loop.
    if (
      summary &&
      r.data.some(
        (m) => m.source === null && !m.enrich_attempted && summary.mod_shas.includes(m.sha1),
      )
    ) {
      await commands.modsEnrichPackMods(reqId);
      const r2 = await commands.modsListInstalled(reqId);
      if (instanceId !== reqId) return;
      if (r2.status === 'ok') r = r2;
    }

    // Fetch ModSummary for every platform-installed mod. Manual / unidentifiable
    // mods (source: null) skip the fetch and stay as a degraded row. Bounded
    // concurrency (not Promise.all over the whole list) keeps a big instance
    // from firing dozens of parallel lookups at once — that burst intermittently
    // tripped rate-limits and made rows flicker into "details unavailable".
    const enriched = await mapLimit(r.data, 6, async (m): Promise<Row> => {
      if (m.source === null || m.project_id === null) {
        return { summary: null, installed: m };
      }
      const p = await commands.modsProject(m.source as ModSource, m.project_id);
      if (p.status === 'ok') {
        return { summary: p.data.summary, installed: m };
      }
      // One retry — a transient hiccup usually clears (the backend caches a
      // concurrent success) so the mod renders normally rather than degraded.
      const retry = await commands.modsProject(m.source as ModSource, m.project_id);
      if (retry.status === 'ok') {
        return { summary: retry.data.summary, installed: m };
      }
      return { summary: null, installed: m };
    });
    // Drop the result if the user switched instances while the per-mod
    // project lookups were in flight — committing here would render the
    // previous instance's mods under the current one.
    if (instanceId !== reqId) return;
    rows = enriched;
    loading = false;
  }

  $effect(() => {
    // biome-ignore lint/correctness/noUnusedVariables: reactive read on instanceId
    const _id = instanceId;
    void refresh();
  });

  let unlisteners: Array<() => void> = [];

  onMount(async () => {
    const handlers = [
      // Install / uninstall change the installed set, so the dep graph must
      // re-resolve too (covers installs/uninstalls from anywhere, incl. the
      // Browse tab). Toggle only flips enabled, which the graph ignores.
      events.modInstalled.listen(() => {
        void refresh();
        reloadGraph();
      }),
      events.modUninstalled.listen(() => {
        void refresh();
        reloadGraph();
      }),
      events.modToggle.listen(() => void refresh()),
    ];
    for (const p of handlers) {
      unlisteners.push(await p);
    }
  });

  onDestroy(() => {
    for (const u of unlisteners) u();
    unlisteners = [];
    if (graphReloadTimer) clearTimeout(graphReloadTimer);
  });

  async function toggle(m: InstalledMod) {
    if (!instanceId) return;
    busy = true;
    error = null;
    const result = m.enabled
      ? await commands.modsDisable(instanceId, m.sha1)
      : await commands.modsEnable(instanceId, m.sha1);
    if (result.status === 'error') {
      error = formatError(result.error);
    } else {
      await refresh();
    }
    busy = false;
  }

  async function uninstall(m: InstalledMod) {
    if (!instanceId) return;
    busy = true;
    error = null;
    const result = await commands.modsUninstall(instanceId, m.sha1);
    if (result.status === 'error') {
      error = formatError(result.error);
    } else {
      await refresh();
      // The removed mod may have been a dependency of another — re-resolve so
      // the tree stops showing it as satisfied.
      reloadGraph();
    }
    busy = false;
  }

  async function switchVersion(row: Row, v: ModVersion) {
    if (!instanceId) return;
    drawerRow = null;
    busy = true;
    error = null;
    // Uninstall the old version, then install the picked one. If the
    // install half fails after the uninstall succeeded the user is left
    // with no mod — surface the error so they can retry.
    const removed = await commands.modsUninstall(instanceId, row.installed.sha1);
    if (removed.status === 'error') {
      error = formatError(removed.error);
      busy = false;
      return;
    }
    const installed = await commands.modsInstallWithDeps(
      instanceId,
      { source: v.source, project_id: v.project_id, version_id: v.version_id },
      [],
    );
    if (installed.status === 'error') {
      pushWarning(get(t)('mods.browse.toastInstallFailed'), [formatError(installed.error)]);
    } else {
      // `installed_dependencies` carries release titles, not mod names — omit
      // them here; the clean reinstall title is the meaningful signal.
      pushSuccess(get(t)('mods.browse.toastInstalledMod', { name: rowDisplayName(row) }));
    }
    busy = false;
    await refresh();
  }

  // Seed the update map from the session cache when the instance changes.
  $effect(() => {
    const id = instanceId;
    if (id) {
      const cached = updateCheckCache.get(id);
      updateChecks = cached ? new Map(cached.map((c) => [c.sha1, c])) : new Map();
    } else {
      updateChecks = new Map();
    }
  });

  async function checkUpdates() {
    if (!instanceId) return;
    checking = true;
    error = null;
    const r = await commands.modsCheckUpdates(instanceId);
    checking = false;
    if (r.status === 'error') {
      error = formatError(r.error);
      showCfBanner = false;
      return;
    }
    updateChecks = new Map(r.data.map((c) => [c.sha1, c]));
    updateCheckCache.set(instanceId, r.data);
    // If a CurseForge mod's check failed, it may be a missing API key —
    // surface the banner only when the key really is absent.
    const cfFailed = r.data.some(
      (c) => c.source === 'curseforge' && c.state.kind === 'check_failed',
    );
    if (cfFailed) {
      const s = await commands.modsGetCurseforgeKeyStatus();
      showCfBanner = s.status === 'ok' && s.data === 'missing';
    } else {
      showCfBanner = false;
    }
  }

  async function applyUpdate(sha1: string, target: ModVersion): Promise<boolean> {
    if (!instanceId) return false;
    const r = await commands.modsUpdateOne(instanceId, sha1, target);
    if (r.status === 'error') {
      error = formatError(r.error);
      return false;
    }
    return true;
  }

  async function updateOne(m: InstalledMod) {
    const check = updateChecks.get(m.sha1);
    if (!instanceId || !check || check.state.kind !== 'update_available') return;
    busy = true;
    error = null;
    const ok = await applyUpdate(m.sha1, check.state.target);
    if (ok) {
      // The stale check no longer applies — drop it; the row's version
      // is now current. The user can re-check to refresh the rest.
      const next = new Map(updateChecks);
      next.delete(m.sha1);
      updateChecks = next;
      updateCheckCache.set(instanceId, [...next.values()]);
    }
    busy = false;
    await refresh();
  }

  const selectedRows = $derived(filtered.filter((r) => selected.has(r.installed.sha1)));
  const selectedUpdatable = $derived(
    selectedRows.filter(
      (r) => updateChecks.get(r.installed.sha1)?.state.kind === 'update_available',
    ),
  );

  async function bulkSetEnabled(enable: boolean) {
    if (!instanceId || selected.size === 0) return;
    busy = true;
    error = null;
    let ok = 0,
      failed = 0;
    for (const r of selectedRows) {
      if (r.installed.enabled === enable) {
        ok++;
        continue;
      }
      const res = enable
        ? await commands.modsEnable(instanceId, r.installed.sha1)
        : await commands.modsDisable(instanceId, r.installed.sha1);
      if (res.status === 'error') failed++;
      else ok++;
    }
    busy = false;
    selected = new Set();
    await refresh();
    if (failed === 0) {
      pushSuccess(
        get(t)(enable ? 'mods.installed.toastEnabled' : 'mods.installed.toastDisabled', {
          count: ok,
        }),
      );
    } else {
      pushWarning(
        get(t)(
          enable ? 'mods.installed.toastEnabledFailed' : 'mods.installed.toastDisabledFailed',
          { count: ok, failed },
        ),
        [],
      );
    }
  }

  async function bulkUpdate() {
    if (!instanceId) return;
    const targets = selectedUpdatable.flatMap((r) => {
      const st = updateChecks.get(r.installed.sha1)?.state;
      return st?.kind === 'update_available' ? [{ sha1: r.installed.sha1, target: st.target }] : [];
    });
    if (targets.length === 0) return;
    busy = true;
    error = null;
    let ok = 0,
      failed = 0;
    for (const t of targets) {
      if (await applyUpdate(t.sha1, t.target)) ok++;
      else failed++;
    }
    updateChecks = new Map();
    updateCheckCache.delete(instanceId);
    busy = false;
    selected = new Set();
    await refresh();
    if (failed === 0) {
      pushSuccess(get(t)('mods.installed.toastUpdated', { count: ok }));
    } else {
      pushWarning(get(t)('mods.installed.toastUpdatedFailed', { count: ok, failed }), []);
    }
  }

  let uninstallPrompt = $state<{
    removing: string[];
    names: string[];
    orphans: OrphanRef[];
  } | null>(null);

  async function requestBulkUninstall() {
    if (!instanceId || selected.size === 0) return;
    const removing = selectedRows.map((r) => r.installed.sha1);
    const names = selectedRows.map((r) => rowDisplayName(r));
    const r = await commands.modsFindOrphans(instanceId, removing);
    const orphans = r.status === 'ok' ? r.data : [];
    uninstallPrompt = { removing, names, orphans };
  }

  async function confirmBulkUninstall(alsoRemove: string[]) {
    if (!instanceId || !uninstallPrompt) return;
    const all = [...uninstallPrompt.removing, ...alsoRemove];
    uninstallPrompt = null;
    busy = true;
    error = null;
    let ok = 0,
      failed = 0;
    for (const sha1 of all) {
      const res = await commands.modsUninstall(instanceId, sha1);
      if (res.status === 'error') failed++;
      else ok++;
    }
    busy = false;
    selected = new Set();
    // Removed mods would otherwise linger as stale roots in the dep tree —
    // invalidate the cached graph and re-resolve (mirrors install-from-tree).
    if (instanceId) {
      depGraphCache.delete(instanceId);
      void loadGraph(instanceId);
    }
    await refresh();
    if (failed === 0) {
      pushSuccess(get(t)('mods.installed.toastUninstalled', { count: ok }));
    } else {
      pushWarning(get(t)('mods.installed.toastUninstalledFailed', { count: ok, failed }), []);
    }
  }

  async function updateAll() {
    if (!instanceId) return;
    const targets = [...updateChecks.values()].flatMap((c) =>
      c.state.kind === 'update_available' ? [{ sha1: c.sha1, target: c.state.target }] : [],
    );
    if (targets.length === 0) return;
    busy = true;
    error = null;
    let ok = 0;
    let failed = 0;
    for (const t of targets) {
      if (await applyUpdate(t.sha1, t.target)) ok++;
      else failed++;
    }
    // Every check is now stale (versions moved) — clear and let the user
    // re-check.
    updateChecks = new Map();
    updateCheckCache.delete(instanceId);
    busy = false;
    await refresh();
    if (failed === 0) {
      pushSuccess(get(t)('mods.installed.toastUpdated', { count: ok }));
    } else {
      pushWarning(get(t)('mods.installed.toastUpdatedFailed', { count: ok, failed }), []);
    }
  }
</script>

<div class="p-3">
  <div class="mb-2 space-y-2">
    {#if totalCount > 0}
      <div class="text-xs text-muted flex gap-3">
        <span
          >{$t('mods.installed.statsTotal')}
          <span class="font-medium text-secondary">{totalCount}</span></span
        >
        <span
          >{$t('mods.installed.statsEnabled')}
          <span class="font-medium text-success">{enabledCount}</span></span
        >
        <span
          >{$t('mods.installed.statsDisabled')}
          <span class="font-medium text-secondary">{disabledCount}</span></span
        >
      </div>
    {/if}
    <div class="flex flex-wrap gap-2 items-center">
      <input
        type="search"
        placeholder={$t('mods.installed.filterPlaceholder')}
        aria-label={$t('mods.installed.filterAriaLabel')}
        class="flex-1 border border-border-emphasis rounded px-3 py-1.5 text-sm"
        bind:value={filter}
      />
      <label class="text-xs text-secondary inline-flex items-center gap-1">
        {$t('mods.installed.sortLabel')}
        <select bind:value={sortBy} class="border rounded px-2 py-1 text-xs bg-surface">
          <option value="name-asc">{$t('mods.installed.sortNameAsc')}</option>
          <option value="name-desc">{$t('mods.installed.sortNameDesc')}</option>
          <option value="recent">{$t('mods.installed.sortRecent')}</option>
          <option value="source">{$t('mods.installed.sortSource')}</option>
        </select>
      </label>
      <button
        type="button"
        class="btn-secondary btn-xs"
        disabled={busy || checking || rows.length === 0}
        onclick={checkUpdates}
      >
        {checking ? $t('mods.card.checking') : $t('mods.installed.checkUpdates')}
      </button>
      <button
        type="button"
        class="btn-secondary btn-xs"
        disabled={graphLoading}
        onclick={recheckDeps}
      >
        {graphLoading ? $t('mods.installed.resolvingDeps') : $t('mods.installed.recheckDeps')}
      </button>
      {#if updateCount > 0}
        <button type="button" class="btn-warning btn-xs" disabled={busy} onclick={updateAll}>
          {$t('mods.installed.updateAll', { count: updateCount })}
        </button>
      {/if}
    </div>
    {#if totalCount > 0}
      <div
        role="radiogroup"
        aria-label={$t('mods.installed.filterGroupAriaLabel')}
        tabindex={-1}
        class="flex gap-1 text-xs"
        onkeydown={handleFilterKey}
      >
        <button
          type="button"
          role="radio"
          aria-checked={enabledFilter === 'all'}
          tabindex={enabledFilter === 'all' ? 0 : -1}
          data-value="all"
          class="btn-secondary btn-xs"
          class:bg-accent-soft={enabledFilter === 'all'}
          class:text-accent={enabledFilter === 'all'}
          class:font-medium={enabledFilter === 'all'}
          onclick={() => (enabledFilter = 'all')}
        >
          {$t('mods.installed.filterAll', { count: totalCount })}
        </button>
        <button
          type="button"
          role="radio"
          aria-checked={enabledFilter === 'enabled'}
          tabindex={enabledFilter === 'enabled' ? 0 : -1}
          data-value="enabled"
          class="btn-secondary btn-xs"
          class:bg-success-bg={enabledFilter === 'enabled'}
          class:text-success={enabledFilter === 'enabled'}
          class:font-medium={enabledFilter === 'enabled'}
          onclick={() => (enabledFilter = 'enabled')}
        >
          {$t('mods.installed.filterEnabled', { count: enabledCount })}
        </button>
        <button
          type="button"
          role="radio"
          aria-checked={enabledFilter === 'disabled'}
          tabindex={enabledFilter === 'disabled' ? 0 : -1}
          data-value="disabled"
          class="btn-secondary btn-xs"
          class:bg-subtle={enabledFilter === 'disabled'}
          class:text-secondary={enabledFilter === 'disabled'}
          class:font-medium={enabledFilter === 'disabled'}
          onclick={() => (enabledFilter = 'disabled')}
        >
          {$t('mods.installed.filterDisabled', { count: disabledCount })}
        </button>
      </div>
    {/if}
  </div>

  {#if error}
    <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2 mb-2">
      {error}
    </div>
  {/if}
  {#if showCfBanner}
    <CurseForgeKeyBanner onOpenSettings={() => (settingsOpen.value = { tab: 'curseforge' })} />
  {/if}

  {#if !instanceId}
    <div class="text-placeholder text-sm py-8 text-center">
      {$t('mods.installed.pickInstanceFirst')}
    </div>
  {:else if loading && rows.length === 0}
    <div class="text-placeholder text-sm py-8 text-center">{$t('mods.installed.loading')}</div>
  {:else if rows.length === 0}
    <div class="text-placeholder text-sm py-8 text-center">
      {$t('mods.installed.empty')}
    </div>
  {:else}
    <div class="border border-border-subtle rounded overflow-hidden">
      <!-- Persistent selection header. The select-all checkbox sits in the
           same left column as the per-row checkboxes (conventional placement);
           bulk actions appear inline here when ≥1 mod is selected, so toggling
           selection never shifts the rows below. -->
      <div
        class="flex items-center gap-3 px-3 py-2 border-b border-border-subtle bg-subtle/40 text-sm"
      >
        <input
          type="checkbox"
          class="flex-shrink-0"
          aria-label={$t('mods.installed.selectAll')}
          checked={allSelected}
          indeterminate={selected.size > 0 && !allSelected}
          onchange={(e) => toggleSelectAll((e.currentTarget as HTMLInputElement).checked)}
        />
        {#if selected.size > 0}
          <span class="font-medium text-accent"
            >{$t('mods.installed.selectedCount', { count: selected.size })}</span
          >
          <div data-testid="bulk-bar" class="ml-auto flex items-center gap-1">
            <button
              type="button"
              class="btn-secondary btn-xs"
              disabled={busy}
              onclick={() => bulkSetEnabled(true)}>{$t('mods.card.enable')}</button
            >
            <button
              type="button"
              class="btn-secondary btn-xs"
              disabled={busy}
              onclick={() => bulkSetEnabled(false)}>{$t('mods.card.disable')}</button
            >
            <button
              type="button"
              class="btn-secondary btn-xs"
              disabled={busy || selectedUpdatable.length === 0}
              title={selectedUpdatable.length === 0 ? $t('mods.installed.bulkUpdateTitle') : ''}
              onclick={bulkUpdate}>{$t('mods.card.update')}</button
            >
            <button
              type="button"
              class="btn-ghost-danger btn-xs"
              disabled={busy}
              onclick={requestBulkUninstall}>{$t('mods.card.uninstall')}</button
            >
            <button type="button" class="btn-ghost btn-xs" onclick={() => (selected = new Set())}
              >{$t('mods.installed.bulkClear')}</button
            >
          </div>
        {:else}
          <span class="text-muted text-xs">{$t('mods.installed.bulkHint')}</span>
        {/if}
      </div>
      {#each paged as row (row.installed.sha1)}
        {#if row.summary}
          {@const rowKey = modKey(
            row.installed.source,
            row.installed.project_id,
            row.installed.sha1,
          )}
          {@const root = rootBySha.get(row.installed.sha1)}
          {@const counts = depCounts(root)}
          {@const reqBy = requiredBy.get(row.installed.project_id ?? '') ?? []}
          <div
            data-mod-key={rowKey}
            data-mod-row={rowKey}
            class:bg-highlight={hoveredKey === rowKey}
            onmouseenter={() => (hoveredKey = rowKey)}
            onmouseleave={() => (hoveredKey = null)}
            role="group"
          >
            <ModCard
              layout="list"
              summary={row.summary}
              installed={row.installed}
              onInstall={() => {}}
              onOpenDetail={() => (drawerRow = row)}
              onToggle={() => toggle(row.installed)}
              onUninstall={() => uninstall(row.installed)}
              updateState={updateChecks.get(row.installed.sha1)?.state ?? null}
              onUpdate={() => updateOne(row.installed)}
              {checking}
              packChip={packSummary && packSummary.mod_shas.includes(row.installed.sha1)
                ? packSummary.project_name
                : null}
              selectable={true}
              selected={selected.has(row.installed.sha1)}
              onSelectChange={(c) => toggleSelect(row.installed.sha1, c)}
            />
            <div class="flex items-center gap-2 px-3 pb-1 text-xs">
              {#if graphLoading && !root}
                <span class="text-placeholder">{$t('mods.installed.resolvingShort')}</span>
              {:else}
                {#if counts.total > 0}
                  <button
                    type="button"
                    class="px-2 py-0.5 rounded bg-accent-soft text-accent"
                    onclick={() => toggleExpand(row.installed.sha1)}
                  >
                    {expanded.has(row.installed.sha1) ? '▾' : '▸'}
                    {$t('mods.installed.depCount', { count: counts.total })}{counts.missing > 0
                      ? ` · ${$t('mods.installed.depMissing', { count: counts.missing })}`
                      : ''}
                  </button>
                {/if}
                {#if reqBy.length > 0}
                  <button
                    type="button"
                    class="px-2 py-0.5 rounded bg-subtle text-secondary"
                    onclick={() => toggleExpand(row.installed.sha1)}
                    >{$t('mods.installed.requiredByCount', { count: reqBy.length })}</button
                  >
                {/if}
              {/if}
            </div>
            {#if expanded.has(row.installed.sha1) && root}
              <div class="px-4 pb-3 bg-subtle/40">
                {#if root.required.length > 0}
                  <div class="text-[10px] uppercase tracking-wide text-muted mt-1">
                    {$t('mods.installed.sectionRequires')}
                  </div>
                  <DepTree
                    nodes={root.required}
                    {hoveredKey}
                    onHover={(k) => (hoveredKey = k)}
                    onInstall={installDepNode}
                    onAdd={installDepNode}
                    onJump={jumpToMod}
                  />
                {/if}
                {#if root.optional.length > 0}
                  <div class="text-[10px] uppercase tracking-wide text-muted mt-2">
                    {$t('mods.installed.sectionRecommended')}
                  </div>
                  <DepTree
                    nodes={root.optional}
                    {hoveredKey}
                    onHover={(k) => (hoveredKey = k)}
                    onInstall={installDepNode}
                    onAdd={installDepNode}
                    onJump={jumpToMod}
                  />
                {/if}
                {#if reqBy.length > 0}
                  <div class="text-[10px] uppercase tracking-wide text-muted mt-2">
                    {$t('mods.installed.sectionRequiredBy')}
                  </div>
                  <div class="text-xs text-secondary">{reqBy.join(', ')}</div>
                {/if}
              </div>
            {/if}
          </div>
        {:else}
          <!-- No ModSummary for this row. Three cases: a hand-dropped
               "manual mod" (source null); a modpack override-bundled jar
               hash-enrichment couldn't identify ("from modpack" + 📦); or a
               real platform mod whose summary lookup failed transiently —
               that one keeps its platform identity, so don't call it manual. -->
          {@const fromPack = !!packSummary && packSummary.mod_shas.includes(row.installed.sha1)}
          {@const isPlatform = row.installed.source !== null}
          {@const sourceLabel = row.installed.source === 'curseforge' ? 'CurseForge' : 'Modrinth'}
          {@const mKey = modKey(row.installed.source, row.installed.project_id, row.installed.sha1)}
          <div
            class="flex items-center gap-3 px-3 py-2 border-b border-border-subtle bg-surface"
            data-testid="manual-mod-row"
            data-mod-key={mKey}
            data-mod-row={mKey}
            class:bg-highlight={hoveredKey === mKey}
            onmouseenter={() => (hoveredKey = mKey)}
            onmouseleave={() => (hoveredKey = null)}
            role="group"
          >
            <input
              type="checkbox"
              class="flex-shrink-0"
              checked={selected.has(row.installed.sha1)}
              aria-label={$t('mods.installed.selectModAriaLabel', {
                filename: row.installed.filename,
              })}
              onchange={(e) =>
                toggleSelect(row.installed.sha1, (e.currentTarget as HTMLInputElement).checked)}
            />
            <div
              class="w-8 h-8 rounded bg-subtle flex items-center justify-center text-placeholder text-xs flex-shrink-0"
              aria-hidden="true"
            >
              ◆
            </div>
            <div class="flex-1 min-w-0">
              <div class="font-medium text-primary truncate">
                {isPlatform && !fromPack ? row.installed.name : row.installed.filename}
              </div>
              <div class="text-xs text-muted truncate">
                {fromPack
                  ? $t('mods.installed.fromModpack')
                  : isPlatform
                    ? `${sourceLabel} · ${$t('mods.installed.detailsUnavailable')}`
                    : $t('mods.installed.manualMod')} · {row.installed.enabled
                  ? $t('mods.installed.enabledStatus')
                  : $t('mods.installed.disabledStatus')}
              </div>
            </div>
            <div class="flex items-center gap-1 flex-shrink-0">
              {#if fromPack && packSummary}
                <span
                  class="text-xs px-2 py-0.5 rounded bg-accent-soft text-accent"
                  title={$t('mods.card.fromModpackTitle', { name: packSummary.project_name })}
                  >📦 {packSummary.project_name}</span
                >
              {/if}
              <button
                type="button"
                class="btn-secondary btn-xs"
                disabled={busy}
                onclick={() => toggle(row.installed)}
                >{row.installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}</button
              >
              <button
                type="button"
                class="btn-ghost-danger btn-xs"
                disabled={busy}
                onclick={() => uninstall(row.installed)}>{$t('mods.card.uninstall')}</button
              >
            </div>
          </div>
        {/if}
      {/each}
    </div>

    <!-- Pagination footer. Selection + the dep graph span the whole filtered
         set; only the rendered rows are paged. -->
    <div class="flex items-center justify-between gap-3 mt-2 text-sm flex-wrap">
      <span class="inline-flex items-center gap-2">
        <span class="text-muted">{$t('mods.pageSize.perPage')}</span>
        {#each PAGE_SIZES as n (n)}
          <button
            type="button"
            class="px-0.5 {pageSize === n
              ? 'text-primary font-semibold'
              : 'text-secondary hover:text-primary'}"
            aria-pressed={pageSize === n}
            onclick={() => (pageSize = n)}
          >
            {n}
          </button>
        {/each}
      </span>
      {#if pageCount > 1}
        <span class="inline-flex items-center gap-2 text-secondary">
          <button
            type="button"
            class="btn-secondary btn-xs"
            disabled={page === 0}
            onclick={() => (page = Math.max(0, page - 1))}>{$t('mods.installed.prevPage')}</button
          >
          <span class="text-muted"
            >{$t('mods.browse.pageOf', { page: page + 1, total: pageCount })}</span
          >
          <button
            type="button"
            class="btn-secondary btn-xs"
            disabled={page >= pageCount - 1}
            onclick={() => (page = Math.min(pageCount - 1, page + 1))}
            >{$t('mods.installed.nextPage')}</button
          >
        </span>
      {/if}
    </div>
  {/if}

  {#if drawerRow && drawerRow.installed.source && drawerRow.installed.project_id && instanceId}
    <ModDetailModal
      source={drawerRow.installed.source as ModSource}
      projectId={drawerRow.installed.project_id}
      {mcVersion}
      {loader}
      installedVersionId={drawerRow.installed.version_id}
      onClose={() => (drawerRow = null)}
      onInstall={(v) => {
        if (drawerRow) void switchVersion(drawerRow, v);
      }}
    />
  {/if}

  {#if uninstallPrompt}
    <OrphanUninstallDialog
      removingNames={uninstallPrompt.names}
      orphans={uninstallPrompt.orphans}
      onCancel={() => (uninstallPrompt = null)}
      onConfirm={confirmBulkUninstall}
    />
  {/if}
</div>
