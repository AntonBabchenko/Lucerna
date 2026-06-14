import { commands, type DepProjectRef, type PreflightReport } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { preflightCache } from './preflight-cache';

// ---------------------------------------------------------------------------
// Pure helpers (no Svelte runtime — safe in Vitest)
// ---------------------------------------------------------------------------

/**
 * Build the set of overlay keys (`${source}:${project_id}`) for every
 * `version_out_of_range` violation that has a `provider_project`.
 *
 * Key format mirrors `DepTree.svelte`'s `keyOf`: `${n.source}:${n.project_id}`.
 * For modrinth refs the key is `modrinth:${project_id}`.
 * For curseforge refs the key is `curseforge:${mod_id}` (the DepTreeNode
 * stores the numeric mod_id as its `project_id` string).
 */
export function toOverlayKeys(report: PreflightReport): Set<string> {
  const out = new Set<string>();
  for (const v of report.violations) {
    if (v.kind !== 'version_out_of_range' || v.provider_project === null) continue;
    out.add(depProjectRefKey(v.provider_project));
  }
  return out;
}

function depProjectRefKey(ref: DepProjectRef): string {
  if (ref.source === 'modrinth') {
    return `modrinth:${ref.project_id}`;
  }
  // curseforge: DepTreeNode.project_id holds the stringified mod_id
  return `curseforge:${ref.mod_id}`;
}

/**
 * True when the report contains at least one violation — i.e. there is
 * something that will block or warn on launch.
 */
export function hasBlocking(report: PreflightReport): boolean {
  return report.violations.length > 0;
}

// ---------------------------------------------------------------------------
// Composable factory
// ---------------------------------------------------------------------------

/**
 * Owns the pre-flight report for the active instance. Mirrors `createDepGraph`
 * from `dep-graph.svelte.ts`: seeds from the per-instance LRU cache on
 * instance switch, kicks off a background `instanceDependencyPreflight` call,
 * race-guards stale results, and exposes `invalidate()` + `dispose()`.
 *
 * Fail-open: if the command errors, `error` is set and the previous report (or
 * null) is retained — never throws, never blocks launch on its own.
 */
export function createPreflight(getInstanceId: () => string | null) {
  let report = $state<PreflightReport | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  async function reloadNow() {
    const id = getInstanceId();
    if (!id) return;
    loading = true;
    error = null;
    const r = await commands.instanceDependencyPreflight(id);
    if (getInstanceId() !== id) {
      // Instance switched while we were in flight — discard stale result.
      loading = false;
      return;
    }
    loading = false;
    if (r.status === 'ok') {
      report = r.data;
      preflightCache.set(id, r.data);
    } else {
      error = formatError(r.error);
      // Fail-open: leave report as-is (null or last known good).
    }
  }

  function invalidate() {
    const id = getInstanceId();
    if (id) {
      preflightCache.delete(id);
      void reloadNow();
    }
  }

  // Seed from cache on instance change + kick off a background pre-flight.
  // Wrapped in $effect.root so the factory is unit-testable without a Svelte
  // runtime and torn down via dispose() on component unmount.
  let stopEffects: (() => void) | null = null;
  try {
    stopEffects = $effect.root(() => {
      $effect(() => {
        const id = getInstanceId();
        if (!id) {
          report = null;
          return;
        }
        const cached = preflightCache.get(id);
        if (cached) {
          report = cached;
        } else {
          report = null;
          void reloadNow();
        }
      });
    });
  } catch {
    /* no Svelte runtime (vitest) — effect inert, which is what unit tests want */
  }

  return {
    get report() {
      return report;
    },
    get loading() {
      return loading;
    },
    get error() {
      return error;
    },
    reload() {
      void reloadNow();
    },
    invalidate,
    dispose() {
      stopEffects?.();
    },
  };
}
