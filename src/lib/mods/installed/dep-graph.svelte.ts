import { tick } from 'svelte';
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import {
  commands,
  type DependencyGraph,
  type DepRoot,
  type DepTreeNode,
  type LoaderKind,
} from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
import { depGraphCache } from '../dep-graph-cache';
import type { Row } from './installed-data.svelte';
import { modKey, rowDisplayName } from './row-utils';

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
          if (n.status === 'satisfied' && !seen.has(n.project_id)) {
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

  // Combine: project_id → display names. Cheap join of the two maps above; the
  // graph root's own `name` is the registry/release title, so prefer the
  // resolved project name from rows.
  const requiredBy = $derived.by(() => {
    const out = new Map<string, string[]>();
    for (const [pid, shas] of requiredByShas) {
      out.set(
        pid,
        shas.map((sha) => nameBySha.get(sha) ?? rootBySha.get(sha)?.name ?? sha),
      );
    }
    return out;
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

  // Set of root sha1s with >=1 missing required dep — feeds the "issues"
  // quick-filter and the attention bar.
  const missingShas = $derived.by(() => {
    const out = new Set<string>();
    for (const r of graph?.roots ?? []) {
      if (depCounts(r).missing > 0) out.add(r.sha1);
    }
    return out;
  });

  async function reloadGraphNow() {
    const id = getInstanceId();
    if (!id) return;
    graphLoading = true;
    const r = await commands.modsDependencyGraph(id);
    if (getInstanceId() !== id) return; // instance-switch race guard
    graphLoading = false;
    if (r.status === 'ok') {
      graph = r.data;
      depGraphCache.set(id, r.data);
    }
  }

  // Force a fresh resolve after the installed SET changes (install/uninstall).
  // Debounced: a bulk uninstall emits one event per mod; collapse the burst.
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

  async function jumpToMod(node: DepTreeNode) {
    const key = `${node.source}:${node.project_id}`;
    hoveredKey = key;
    const filtered = ctx.getFiltered();
    const idx = filtered.findIndex(
      (r) => modKey(r.installed.source, r.installed.project_id, r.installed.sha1) === key,
    );
    if (idx < 0) return;
    ctx.setPage(Math.floor(idx / ctx.getPageSize()));
    await tick();
    if (typeof document !== 'undefined') {
      const el = document.querySelector(`[data-mod-row="${key}"]`);
      (el as HTMLElement | null)?.scrollIntoView?.({ behavior: 'smooth', block: 'center' });
    }
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
    const res = await commands.modsInstallWithDeps(
      id,
      { source: primary.source, project_id: primary.project_id, version_id: primary.version_id },
      [],
    );
    if (res.status === 'error') {
      pushWarning(get(t)('mods.browse.toastInstallFailed'), [formatError(res.error)]);
    } else {
      pushSuccess(get(t)('mods.browse.toastInstalledMod', { name: node.name }));
    }
    busy = false;
    invalidateGraph();
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
        graph = id ? (depGraphCache.get(id) ?? null) : null;
        expanded = new Set();
        hoveredKey = null;
        if (id) void reloadGraphNow();
      });
    });
  } catch {
    /* no Svelte runtime (vitest) — effect inert, which is what unit tests want */
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
    get missingShas() {
      return missingShas;
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
