import {
  commands,
  type DepProjectRef,
  type DepViolation,
  type InstallMissingOutcome,
  type Error as IpcError,
  type ModVersion,
  type PreflightReport,
  type VersionFixPlan,
} from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import type { InstallOpts } from '$lib/tasks/adapters/mod-install';
import { installModWithDeps, updateMod } from '$lib/tasks/adapters/mod-install';
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
    const key = overlayKeyOf(v);
    if (key !== null) out.add(key);
  }
  return out;
}

/** The tree node a violation marks «version mismatch» (its overlay key), or null. */
function overlayKeyOf(v: DepViolation): string | null {
  return isRangeRemediable(v) && v.provider_project !== null
    ? depProjectRefKey(v.provider_project)
    : null;
}

/**
 * The conflict behind an out-of-range tree node — what its «Fix…» asks the
 * planner about. The overlay marks a PROJECT, so a node can be marked for
 * another dependent's range: this dependent's own conflict on the project
 * comes first, else the first one the mark stands for. Null when nothing marks
 * the node (by `toOverlayKeys`' own rule).
 */
export function overlayConflict(
  violations: readonly DepViolation[],
  node: { source: string; project_id: string },
  dependentSha1: string | null,
): DepViolation | null {
  const key = `${node.source}:${node.project_id}`;
  const marking = violations.filter((v) => overlayKeyOf(v) === key);
  return marking.find((v) => v.dependent_sha1 === dependentSha1) ?? marking[0] ?? null;
}

/**
 * True when "install a version that satisfies the declared range" is a sound
 * manual repair for this violation — the panel's «Choose version» picker, whose
 * "fits range" marks come from `modsFilterSatisfying`.
 *
 * Deliberately EXCLUDES `incompatible_installed`: there the range names the
 * versions that clash, so the satisfying set is exactly what must be avoided.
 * That row's repair is the planner (`planVersionFix`), which judges the
 * negation.
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
 * True when the report contains at least one violation AND the instance is not a
 * modpack that is still assembling itself.
 *
 * A pack shipping a completer mod (see `pack_completion`) starts with genuinely
 * unmet mandatory dependencies because its remaining files cannot be
 * redistributed — the pack fetches them on first launch. Blocking there asks the
 * user to decide something they cannot, and hides the fix behind our own gate.
 * The moment the last outstanding file lands, blocking returns on its own.
 */
export function hasBlocking(report: PreflightReport): boolean {
  return report.violations.length > 0 && (report.pack_completion?.outstanding.length ?? 0) === 0;
}

// ---------------------------------------------------------------------------
// Remediation helpers
// ---------------------------------------------------------------------------

/** What the two-sided planner said about one conflict (spec §5.4). */
export type PlanAnswer =
  | { status: 'ready'; plan: VersionFixPlan }
  /** Neither side has a build that fixes it — the honest dead end. */
  | { status: 'dead_end' }
  /** The look itself failed (offline, rate-limited, no CurseForge key): never "no version". */
  | { status: 'failed'; message: string };

/**
 * Ask the planner how to fix the conflict `v` reports — update the dependent to
 * a build that accepts what is installed, or change the dependency to a build
 * the dependent accepts, naming whom that breaks. The network is used only
 * here, on the user's click. Never throws: a call that failed, or a bridge that
 * threw, is `failed` with its reason — a look that could not be made is not a
 * dead end (spec §9).
 */
export async function planVersionFix(instanceId: string, v: DepViolation): Promise<PlanAnswer> {
  try {
    const r = await commands.modsPlanVersionFix(instanceId, v.dependent_sha1, v.dep_id);
    if (r.status === 'error') return { status: 'failed', message: formatError(r.error) };
    const { update_dependent: dependent, change_provider: provider } = r.data;
    return dependent || provider ? { status: 'ready', plan: r.data } : { status: 'dead_end' };
  } catch (e) {
    return { status: 'failed', message: e instanceof Error ? e.message : String(e) };
  }
}

/** Stable per-row key for a violation (matches `PreflightPanel`'s row key). */
export function violationKey(v: DepViolation): string {
  return `${v.dependent_sha1}:${v.dep_id}`;
}

/**
 * Install a user-chosen version for a violation (manual pick / downgrade from
 * the version picker). Never throws; returns the installed `version_number` on
 * success for the toast, and the typed error on failure so the caller can ASK
 * about a build that is not for this instance instead of toasting a failure no
 * retry could fix.
 *
 * The jar to replace is the tracked provider when the preflight knows it
 * (`provider_sha1`), else the build the CALLER found installed for the chosen
 * project (`opts.installedSha1`) — this helper cannot see the instance. With
 * either, the pick is a version switch and goes through `mods_update_one`:
 * one command that downloads the new build before it removes the old one.
 * With neither it is a fresh install.
 */
export async function remediatePickedVersion(
  instanceId: string,
  v: DepViolation,
  chosen: ModVersion,
  opts: InstallOpts & { installedSha1?: string | null } = {},
): Promise<{ ok: boolean; installedVersion?: string; error?: IpcError }> {
  const oldSha1 = v.provider_sha1 ?? opts.installedSha1 ?? null;
  const install: InstallOpts = { allowOffPlatform: opts.allowOffPlatform === true };
  const res = oldSha1
    ? await updateMod(instanceId, chosen.name, oldSha1, chosen, install)
    : await installModWithDeps(
        instanceId,
        chosen.name,
        { source: chosen.source, project_id: chosen.project_id, version_id: chosen.version_id },
        [],
        install,
      );
  return res.status === 'ok'
    ? { ok: true, installedVersion: chosen.version_number }
    : { ok: false, error: res.error };
}

/** What «Install {dep}» on a missing dependency came to. */
export type InstallMissingResult =
  | InstallMissingOutcome
  /** The call failed: typed, never passed off as the backend's `open_search` miss. */
  | { kind: 'failed'; error: IpcError };

/**
 * Resolve + install a missing required dependency by its loader mod-id.
 *
 * `dependentSha1` identifies the mod that DECLARED the dependency. The backend
 * reads that mod's platform metadata first, which names the dependency's
 * project outright, and only then falls back to guessing a slug from the bare
 * id — a guess that fails outright for a slammed id like `forgeconfigapiport`.
 *
 * `open_search` is the backend's own "could not resolve it with confidence".
 * An error is not that: «already installed» or a busy profile read as a miss
 * would send the user to a search for a second copy — so it comes back typed,
 * for the caller to tell apart.
 */
export async function installMissing(
  instanceId: string,
  dependentSha1: string,
  depId: string,
): Promise<InstallMissingResult> {
  const res = await commands.modsInstallMissingRequired(instanceId, dependentSha1, depId);
  return res.status === 'ok' ? res.data : { kind: 'failed', error: res.error };
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
 * race-guards stale results, and exposes `invalidate()`, `check()` + `dispose()`.
 *
 * Fail-open: if the command errors, `error` is set and the previous report (or
 * null) is retained — never blocks launch on its own.
 */
export function createPreflight(getInstanceId: () => string | null) {
  let report = $state<PreflightReport | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  // Every load takes a ticket and only the latest may commit. Loads overlap — a
  // mod event's reload and the Play gate's check — and can answer out of order;
  // the one started later read the later mods folder, so an earlier answer that
  // lands after it is stale, however late it lands.
  let ticket = 0;

  async function load(id: string) {
    const mine = ++ticket;
    loading = true;
    error = null;
    const r = await commands.instanceDependencyPreflight(id);
    if (mine !== ticket) return r; // a newer load owns the state now
    loading = false;
    if (r.status === 'ok') preflightCache.set(id, r.data);
    // A verdict about one profile never lands on another (switched meanwhile).
    if (getInstanceId() !== id) return r;
    if (r.status === 'ok') {
      report = r.data;
    } else {
      error = formatError(r.error);
      // Fail-open: leave report as-is (null or last known good).
    }
    return r;
  }

  async function reloadNow() {
    const id = getInstanceId();
    if (id) await load(id);
  }

  /**
   * Run a FRESH pre-flight for `id` and hand back the raw result — the Play
   * gate's check and the repair's re-check, which must never trust a cached
   * verdict. The answer is committed like any reload (while `id` is still
   * active), so every surface reading this report agrees with the gate.
   */
  function check(id: string) {
    return load(id);
  }

  function invalidate() {
    const id = getInstanceId();
    if (id) {
      preflightCache.delete(id);
      void reloadNow();
    }
  }

  // Seed from cache on instance change + kick off a background pre-flight.
  // Wrapped in $effect.root so the factory works outside a component (the page
  // and the Installed tab own one each) and is torn down via dispose().
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
    /* no reactive runtime to root the effect in — it stays inert; `reload` and
       `check` still work. Under vitest the runtime IS there: the effect seeds
       or loads on its first run (a test's `flushSync` or first await). */
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
    check,
    dispose() {
      stopEffects?.();
    },
  };
}
