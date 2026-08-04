import {
  commands,
  type DepProjectRef,
  type DepViolation,
  type InstallMissingOutcome,
  type LoaderKind,
  type ModVersion,
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
    if (!isRangeRemediable(v) || v.provider_project === null) continue;
    out.add(depProjectRefKey(v.provider_project));
  }
  return out;
}

/**
 * True when "install a version that satisfies the declared range" is the right
 * repair for this violation.
 *
 * Deliberately EXCLUDES `incompatible_installed`: there the range names the
 * versions that clash, so the satisfying set is exactly what must be avoided —
 * remediating it with `modsFilterSatisfying` would install a worse version.
 * That row is informational until a dedicated "pick a version outside the
 * range" flow exists.
 */
export function isRangeRemediable(v: DepViolation): boolean {
  return v.kind === 'version_out_of_range' || v.kind === 'optional_out_of_range';
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
 * Resolve and install the newest version that BOTH is MC/loader-compatible AND
 * satisfies the dependency's declared range, for a single `version_out_of_range`
 * violation that has a `provider_project`.
 *
 * The range check runs in Rust (`mods_filter_satisfying`, backed by
 * `version_range::satisfies`) on the version list we already fetched, so the
 * snapshot-beta case — newest-compatible but out of range — is skipped instead
 * of re-installed.
 *
 * Fail semantics (never throws):
 * - `no-provider`   — no provider project / family to resolve against.
 * - `no-version`    — the platform returned no versions.
 * - `no-satisfying` — versions exist but none satisfy the range (honest dead-end).
 * - `update-failed` — the install/update command errored.
 * On success returns the installed `version_number` for the toast.
 */
export async function remediateViolation(
  instanceId: string,
  v: DepViolation,
  mc: string,
  loader: LoaderKind,
): Promise<{ ok: boolean; reason?: string; installedVersion?: string }> {
  if (v.provider_project === null || v.family === null) {
    return { ok: false, reason: 'no-provider' };
  }
  const { source, projectId } = depRefToIds(v.provider_project);
  const vr = await commands.modsVersions(source, projectId, mc, loader);
  if (vr.status === 'error' || vr.data.length === 0) {
    return { ok: false, reason: 'no-version' };
  }
  // Keep only versions that satisfy the declared range; the first index is the
  // newest satisfying one (modsVersions is newest-first).
  const idx = await commands.modsFilterSatisfying(
    vr.data.map((x) => x.version_number),
    v.needed,
    v.family,
  );
  if (idx.length === 0) {
    return { ok: false, reason: 'no-satisfying' };
  }
  const primary = vr.data[idx[0]];
  const res = v.provider_sha1
    ? await commands.modsUpdateOne(instanceId, v.provider_sha1, primary)
    : await commands.modsInstallWithDeps(
        instanceId,
        { source: primary.source, project_id: primary.project_id, version_id: primary.version_id },
        [],
      );
  return res.status === 'ok'
    ? { ok: true, installedVersion: primary.version_number }
    : { ok: false, reason: 'update-failed' };
}

/**
 * Attempt to remediate every range-remediable violation with a
 * `provider_project` in the given report (see `isRangeRemediable` — an
 * incompatibility is not one of them). Runs sequentially (install-order
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
    if (!isRangeRemediable(v) || v.provider_project === null) continue;
    const result = await remediateViolation(instanceId, v, mc, loader);
    if (result.ok) updated++;
  }
  return updated;
}

/** Stable per-row key for a violation (matches `PreflightPanel`'s row key). */
export function violationKey(v: DepViolation): string {
  return `${v.dependent_sha1}:${v.dep_id}`;
}

/**
 * Install a user-chosen version for a violation (manual pick / downgrade from
 * the version picker). Routes through `mods_update_one` (in-place replace) when
 * the provider jar is tracked, else a fresh install. Never throws; returns the
 * installed `version_number` on success for the toast.
 */
export async function remediatePickedVersion(
  instanceId: string,
  v: DepViolation,
  chosen: ModVersion,
): Promise<{ ok: boolean; installedVersion?: string }> {
  const res = v.provider_sha1
    ? await commands.modsUpdateOne(instanceId, v.provider_sha1, chosen)
    : await commands.modsInstallWithDeps(
        instanceId,
        { source: chosen.source, project_id: chosen.project_id, version_id: chosen.version_id },
        [],
      );
  return res.status === 'ok'
    ? { ok: true, installedVersion: chosen.version_number }
    : { ok: false };
}

/**
 * Resolve + install a missing required dependency by its loader mod-id.
 * Fail-safe: any IPC error degrades to an `open_search` outcome so the caller
 * always has an actionable next step (never throws).
 */
export async function installMissing(
  instanceId: string,
  depId: string,
): Promise<InstallMissingOutcome> {
  const res = await commands.modsInstallMissingRequired(instanceId, depId);
  if (res.status === 'ok') return res.data;
  return { kind: 'open_search', query: depId };
}

// ---------------------------------------------------------------------------
// Launch decision helper (pure, testable outside Svelte components)
// ---------------------------------------------------------------------------

/**
 * What the launch flow should do given the raw result of
 * `instanceDependencyPreflight`.
 *
 * Three states, not two. A check that could not run and a check that passed
 * are different facts, and collapsing them is how a detector comes to claim
 * "no problems" when it never ran. `unknown` still launches — a failed
 * check must never block the game (maintainer decision, 2026-08-03) — but the
 * caller now has to handle it explicitly, which is what makes it surfaceable.
 */
export type LaunchDecision =
  | { kind: 'launch' }
  | { kind: 'gate'; report: PreflightReport }
  | { kind: 'unknown'; error: unknown };

/**
 * Decide what the launch flow should do:
 * - `gate`    — blocking violations found; show the gate dialog.
 * - `launch`  — checked, nothing blocking.
 * - `unknown` — the check itself failed. Proceed, but say so.
 */
export function decideLaunch(
  preflightResult: { status: 'ok'; data: PreflightReport } | { status: 'error'; error: unknown },
): LaunchDecision {
  if (preflightResult.status !== 'ok') {
    return { kind: 'unknown', error: preflightResult.error };
  }
  return hasBlocking(preflightResult.data)
    ? { kind: 'gate', report: preflightResult.data }
    : { kind: 'launch' };
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
