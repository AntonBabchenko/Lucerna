import {
  commands,
  type DepProjectRef,
  type DepViolation,
  type LoaderKind,
  type PreflightReport,
} from '$lib/ipc/bindings';
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
// Remediation helpers
// ---------------------------------------------------------------------------

/**
 * Extract a `{ source, project_id }` pair from a `DepProjectRef` in the same
 * string key format used by `depProjectRefKey` (curseforge uses `mod_id`).
 */
function depRefToIds(ref: DepProjectRef): { source: 'modrinth' | 'curseforge'; projectId: string } {
  if (ref.source === 'modrinth') {
    return { source: 'modrinth', projectId: ref.project_id };
  }
  return { source: 'curseforge', projectId: String(ref.mod_id) };
}

/**
 * Attempt to resolve and install the newest compatible version for a single
 * `version_out_of_range` violation that has a `provider_project`.
 *
 * v1 simplification: picks `vr.data[0]` (modsVersions returns newest-compatible-
 * first, identical to the installDepNode strategy). A rare "version too high"
 * case won't be fixed, but the post-update re-check will surface it honestly.
 *
 * Fail semantics: returns `{ ok: false }` on any IPC error or when there is no
 * resolvable version — never throws.
 */
export async function remediateViolation(
  instanceId: string,
  v: DepViolation,
  mc: string,
  loader: LoaderKind,
): Promise<{ ok: boolean; reason?: string }> {
  if (v.provider_project === null) {
    return { ok: false, reason: 'no-provider' };
  }
  const { source, projectId } = depRefToIds(v.provider_project);
  const vr = await commands.modsVersions(source, projectId, mc, loader);
  if (vr.status === 'error' || vr.data.length === 0) {
    return { ok: false, reason: 'no-version' };
  }
  const primary = vr.data[0];
  const res = v.provider_sha1
    ? await commands.modsUpdateOne(instanceId, v.provider_sha1, primary)
    : await commands.modsInstallWithDeps(
        instanceId,
        { source: primary.source, project_id: primary.project_id, version_id: primary.version_id },
        [],
      );
  return {
    ok: res.status === 'ok',
    reason: res.status === 'ok' ? undefined : 'update-failed',
  };
}

/**
 * Attempt to remediate all `version_out_of_range` violations with a
 * `provider_project` in the given report. Runs sequentially (install-order
 * safety). Returns the number of violations that were successfully updated.
 */
export async function remediateAll(
  instanceId: string,
  report: PreflightReport,
  mc: string,
  loader: LoaderKind,
): Promise<number> {
  let updated = 0;
  for (const v of report.violations) {
    if (v.kind !== 'version_out_of_range' || v.provider_project === null) continue;
    const result = await remediateViolation(instanceId, v, mc, loader);
    if (result.ok) updated++;
  }
  return updated;
}

// ---------------------------------------------------------------------------
// Launch decision helper (pure, testable outside Svelte components)
// ---------------------------------------------------------------------------

/**
 * Decide what the launch flow should do given the raw result of
 * `instanceDependencyPreflight`. Returns:
 * - `'gate'`   — block launch and show the gate dialog (blocking violations found)
 * - `'launch'` — proceed with launch immediately (no violations, or check failed)
 */
export function decideLaunch(
  preflightResult: { status: 'ok'; data: PreflightReport } | { status: 'error'; error: unknown },
): 'gate' | 'launch' {
  if (preflightResult.status !== 'ok') return 'launch'; // fail-open
  return hasBlocking(preflightResult.data) ? 'gate' : 'launch';
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
