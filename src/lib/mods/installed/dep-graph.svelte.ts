import { tick } from 'svelte';
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import {
  commands,
  type DependencyGraph,
  type DepRoot,
  type DepTreeNode,
  type LoaderKind,
  type ModSource,
} from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { modWriteReason } from '$lib/mods/mod-ops.svelte';
import { installModWithDeps } from '$lib/tasks/adapters/mod-install';
import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
import { depGraphCache } from '../dep-graph-cache';
import type { Row } from './installed-data.svelte';
import { modKey, rowDisplayName } from './row-utils';
import { depsLines } from './update-review';

export type RequiredByEntry = { name: string; source: ModSource; projectId: string; sha1: string };

const reachedAll = (nodes: readonly DepTreeNode[]): boolean =>
  nodes.every((n) => n.deps_unknown !== 'unreachable' && reachedAll(n.children));

/**
 * Whether `graph` may be kept for the session. A mod whose dependencies are unknown because the
 * platform could not be reached (`unreachable`: offline, rate-limited, no usable key) is unknown
 * only for now — a graph holding one, at a root or any installed node below, is shown but not
 * kept, so the next open asks again (cheap: the backend keeps the versions it did get). An
 * `unidentified` version is the platform's own answer and stays until the mods change.
 */
export function isSettledGraph(graph: DependencyGraph): boolean {
  return graph.roots.every(
    (r) => r.deps_unknown !== 'unreachable' && reachedAll(r.required) && reachedAll(r.optional),
  );
}

export type DepGraphCtx = {
  getMcVersion: () => string | null;
  getLoader: () => LoaderKind | null;
  refresh: () => Promise<void>;
  getFiltered: () => Row[];
  setPage: (n: number) => void;
  getPageSize: () => number;
};

// Owns the dependency graph for the active instance. The graph loads in the
// background (Strategy C) and never blocks the mod list; it is seeded from a
// per-instance session cache on switch. `requiredBy` is split so the expensive
// recursive walk only re-runs when the graph changes — the cheap sha1→name map
// re-runs on row changes.
export function createDepGraph(
  getInstanceId: () => string | null,
  getRows: () => Row[],
  ctx: DepGraphCtx,
) {
  let graph = $state<DependencyGraph | null>(null);
  let graphLoading = $state(false);
  let expanded = $state<Set<string>>(new Set());
  let hoveredKey = $state<string | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const rootBySha = $derived(new Map((graph?.roots ?? []).map((r) => [r.sha1, r])));

  // Cheap: sha1 → resolved display name. Re-runs on row changes only.
  const nameBySha = $derived(new Map(getRows().map((r) => [r.installed.sha1, rowDisplayName(r)])));

  // Expensive recursive walk: project_id → list of root sha1s that require it.
  // Depends ONLY on `graph`, so a re-sort or row mutation does not re-walk.
  const requiredByShas = $derived.by(() => {
    const map = new Map<string, string[]>();
    for (const r of graph?.roots ?? []) {
      const seen = new Set<string>();
      const walk = (ns: DepTreeNode[]) => {
        for (const n of ns) {
          // Present AND declared required — the same pair the old `satisfied`
          // value stood for. An optional child nested under a required one must
          // not make its parent read as "required by".
          if (n.installed && n.declared === 'required' && !seen.has(n.project_id)) {
            seen.add(n.project_id);
            map.set(n.project_id, [...(map.get(n.project_id) ?? []), r.sha1]);
          }
          if (!n.cycle) walk(n.children);
        }
      };
      walk(r.required);
    }
    return map;
  });

  // project_id -> the installed mods that require it, as clickable/hoverable
  // entries (name + identity keys), preferring the resolved row name over the
  // graph root's release-title name.
  const requiredBy = $derived.by(() => {
    const out = new Map<string, RequiredByEntry[]>();
    for (const [pid, shas] of requiredByShas) {
      out.set(
        pid,
        shas.flatMap((sha) => {
          const root = rootBySha.get(sha);
          if (!root) return [];
          return [
            {
              name: nameBySha.get(sha) ?? root.name,
              source: root.source,
              projectId: root.project_id,
              sha1: sha,
            },
          ];
        }),
      );
    }
    return out;
  });

  // Distinct REQUIRED projects in the subtree (spec 2026-09-28 §6.3: REI is 3,
  // not 4) — a diamond visits a library twice, and an optional child (with
  // everything under it) or a cycle back to the mod itself is not something it
  // requires. Still no "missing" tally: the graph knows what the platform was
  // told, not what the loader enforces. The pre-flight owns "this is a problem".
  function depCounts(root: DepRoot | undefined) {
    if (!root) return { total: 0 };
    const seen = new Set<string>();
    const walk = (ns: DepTreeNode[]) => {
      for (const n of ns) {
        if (n.declared !== 'required') continue;
        seen.add(`${n.source}:${n.project_id}`);
        if (!n.cycle) walk(n.children);
      }
    };
    walk(root.required);
    seen.delete(`${root.source}:${root.project_id}`);
    return { total: seen.size };
  }

  // Every mod change re-resolves the graph, so loads overlap and may answer out
  // of order. Latest wins: an older answer describes a mod set that is gone, and
  // its `graphLoading = false` would hide the newer load still in flight.
  let loadTicket = 0;
  async function reloadGraphNow() {
    const id = getInstanceId();
    if (!id) return;
    const ticket = ++loadTicket;
    graphLoading = true;
    error = null;
    const r = await commands.modsDependencyGraph(id);
    // Superseded: the newer load owns the result and the spinner.
    if (ticket !== loadTicket) return;
    graphLoading = false; // reset even when the answer is for a profile left meanwhile
    if (getInstanceId() !== id) return;
    if (r.status === 'ok') {
      graph = r.data;
      // Only a settled answer is the session's; one the platform could not be reached for is
      // asked for again on the next open — and never leaves an older graph behind it.
      if (isSettledGraph(r.data)) depGraphCache.set(id, r.data);
      else depGraphCache.delete(id);
    } else {
      // Surface the failure so "Re-check deps" doesn't silently do nothing —
      // the graph load failed (offline / rate-limited). InstalledModsView folds
      // `deps.error` into its aggregate error banner, so setting it here is
      // enough to tell the user the recheck failed instead of no-oping.
      error = formatError(r.error);
    }
  }

  // Force a fresh resolve after the mods change (install, uninstall, and a
  // toggle: the graph is rooted at the ENABLED mods and marks the disabled ones).
  // Debounced: a bulk operation emits one event per mod; collapse the burst.
  let graphReloadTimer: ReturnType<typeof setTimeout> | null = null;
  function reloadGraph() {
    const id = getInstanceId();
    if (!id) return;
    if (graphReloadTimer) clearTimeout(graphReloadTimer);
    graphReloadTimer = setTimeout(() => {
      graphReloadTimer = null;
      const cur = getInstanceId();
      if (cur) {
        depGraphCache.delete(cur);
        void reloadGraphNow();
      }
    }, 150);
  }
  function recheckDeps() {
    reloadGraph();
  }
  // Invalidate the cache + reload immediately (used by bulk uninstall's onMutated).
  function invalidateGraph() {
    const id = getInstanceId();
    if (id) {
      depGraphCache.delete(id);
      void reloadGraphNow();
    }
  }

  function toggleExpand(sha1: string) {
    const next = new Set(expanded);
    if (next.has(sha1)) next.delete(sha1);
    else next.add(sha1);
    expanded = next;
  }

  // Turn to the page holding filtered row `idx` (keyed `key`) and scroll it into view.
  async function showRow(idx: number, key: string): Promise<void> {
    ctx.setPage(Math.floor(idx / ctx.getPageSize()));
    await tick();
    if (typeof document !== 'undefined') {
      const el = document.querySelector(`[data-mod-row="${key}"]`);
      (el as HTMLElement | null)?.scrollIntoView?.({ behavior: 'smooth', block: 'center' });
    }
  }

  // Accepts any mod identity (a DepTreeNode or a required-by entry), so the
  // ↗ jump works from both the dependency tree and the "Required by" list.
  async function jumpToMod(target: { source: ModSource; project_id: string }) {
    const key = `${target.source}:${target.project_id}`;
    hoveredKey = key;
    const filtered = ctx.getFiltered();
    const idx = filtered.findIndex(
      (r) => modKey(r.installed.source, r.installed.project_id, r.installed.sha1) === key,
    );
    if (idx < 0) return;
    await showRow(idx, key);
  }

  // The pre-flight panel knows a dependent by its jar, not its project (a manual
  // jar has none). False = the row is not in the filtered list: the caller
  // decides whether to widen the view and try again.
  async function jumpToSha1(sha1: string): Promise<boolean> {
    const filtered = ctx.getFiltered();
    const idx = filtered.findIndex((r) => r.installed.sha1 === sha1);
    const r = filtered[idx];
    if (!r) return false;
    const key = modKey(r.installed.source, r.installed.project_id, sha1);
    hoveredKey = key;
    await showRow(idx, key);
    return true;
  }

  async function installDepNode(node: DepTreeNode) {
    const id = getInstanceId();
    const mc = ctx.getMcVersion();
    const loader = ctx.getLoader();
    if (!id || !mc || !loader) return;
    busy = true;
    error = null;
    const vr = await commands.modsVersions(node.source, node.project_id, mc, loader);
    if (vr.status === 'error' || vr.data.length === 0) {
      error =
        vr.status === 'error'
          ? formatError(vr.error)
          : get(t)('mods.installed.installDepFailed', { name: node.name });
      busy = false;
      return;
    }
    const primary = vr.data[0];
    const res = await installModWithDeps(
      id,
      node.name,
      { source: primary.source, project_id: primary.project_id, version_id: primary.version_id },
      [],
    );
    if (res.status === 'error') {
      // A refused install says the profile is busy — never that the game runs (plan A9).
      pushWarning(get(t)('mods.browse.toastInstallFailed'), [modWriteReason(res.error)]);
    } else {
      // Its own required dependencies come along: said, never installed silently (D9).
      const tt = get(t);
      pushSuccess(
        tt('mods.browse.toastInstalledMod', { name: node.name }),
        depsLines(tt, [res.data]),
      );
    }
    busy = false;
    // Await the graph re-resolve before refreshing rows so the tree's
    // satisfied/missing state is current when the new rows render (matches the
    // original monolith ordering). reloadGraphNow re-reads the instance + race-guards.
    const cur = getInstanceId();
    if (cur) {
      depGraphCache.delete(cur);
      await reloadGraphNow();
    }
    await ctx.refresh();
  }

  // Seed from cache on instance change + kick off a background resolve, and
  // reset per-instance view state. Wrapped in $effect.root so the factory is
  // unit-testable and torn down via dispose() on component unmount.
  let stopEffects: (() => void) | null = null;
  try {
    stopEffects = $effect.root(() => {
      $effect(() => {
        const id = getInstanceId();
        expanded = new Set();
        hoveredKey = null;
        if (!id) {
          graph = null;
          return;
        }
        const cached = depGraphCache.get(id);
        if (cached) {
          // Reuse the session-cached graph — do NOT re-resolve. Re-resolving on
          // every Installed-tab open / instance switch re-hit the mod platforms
          // (a 429 rate-limit source). The entry is dropped whenever the mods
          // change — every install, removal, toggle or external change, by the
          // always-mounted page for any profile and by this view's own reloads
          // (installDepNode -> invalidateGraph) — and by the explicit "Re-check
          // deps" button, so a stale graph can't persist past a real change. A
          // graph the platform could not be reached for is never cached
          // (`isSettledGraph`), so it is asked for again here.
          graph = cached;
        } else {
          graph = null;
          void reloadGraphNow();
        }
      });
    });
  } catch {
    /* no reactive runtime to root the effect in — it stays inert. Under vitest the runtime IS
       there: the effect runs at a test's first await, and may start a load of its own. */
  }

  return {
    get graph() {
      return graph;
    },
    get graphLoading() {
      return graphLoading;
    },
    get expanded() {
      return expanded;
    },
    get hoveredKey() {
      return hoveredKey;
    },
    set hoveredKey(v: string | null) {
      hoveredKey = v;
    },
    get rootBySha() {
      return rootBySha;
    },
    get requiredBy() {
      return requiredBy;
    },
    get busy() {
      return busy;
    },
    get error() {
      return error;
    },
    depCounts,
    toggleExpand,
    jumpToMod,
    jumpToSha1,
    installDepNode,
    reloadGraph,
    reloadGraphNow,
    recheckDeps,
    invalidateGraph,
    dispose() {
      stopEffects?.();
      if (graphReloadTimer) clearTimeout(graphReloadTimer);
    },
  };
}
