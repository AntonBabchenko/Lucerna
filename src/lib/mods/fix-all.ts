import { get } from 'svelte/store';
import { reasonLines } from '$lib/format/reason-lines';
import { t } from '$lib/i18n';
import {
  commands,
  type DepViolation,
  type Error as IpcError,
  type ModVersion,
  type PreflightReport,
} from '$lib/ipc/bindings';
import { updateMod } from '$lib/tasks/adapters/mod-install';
import { depNameOf, depProjectOf } from './dep-names.svelte';
import { enableModsUnguarded, modWriteReason } from './mod-ops.svelte';
import { decideLaunch, hasBlocking, violationKey } from './preflight.svelte';
import { depDisplayName, type ViolationAction, violationAction } from './violation-view';

// «Fix all» on the «What stops the game» panel and «Fix and launch» at the Play
// gate (spec D5, §6.4): ONE repair, shared. A new function, not an extension of
// the retired `remediateAll`, which knew only the two range kinds. Which rows
// it repairs, and how, is `violationAction`'s answer — the same one the panel's
// buttons and «Fix all (N)» count follow. Sequential on purpose: installs and
// version switches must not race on one mods folder. It claims nothing about
// the result: only a re-run pre-flight says which rows are gone (`countFixed`).

export type FixAllResult = {
  /** Every row a repair was tried for. */
  attempted: DepViolation[];
  /**
   * The rows whose repair step reported success — NOT proof that a row is gone:
   * a dependency can install and still not satisfy it.
   */
  applied: DepViolation[];
  /**
   * Why the steps that failed failed, worded for the user — a busy profile as
   * busy (`modWriteReason`) — each reason once, after the mods whose fix it
   * stopped: «Moonlight Lib, ImmediatelyFast: …» (`reasonLines`, the grouping
   * the update report uses). «Fixed 0 of 3» alone cannot tell a held profile
   * from three unrelated failures, and a reason alone does not say which fix it
   * stopped (plan §5c V3). A row the repair had no fix for (no build breaks
   * nothing, an unidentified jar) is no failed step and gives none: the row that
   * stays says what is wrong.
   */
  reasons: string[];
};

/** A step that failed: the mod it would have fixed — installed, switched on or switched to
 *  another build — and why it could not. */
type Failure = { name: string; reason: string };
type Run = { attempted: DepViolation[]; applied: DepViolation[]; failures: Failure[] };

type PreflightResult = Parameters<typeof decideLaunch>[0];

/**
 * Apply every automatic repair in `report`: switch disabled providers on (with
 * the disabled mods they need), install missing dependencies (each with its
 * own required closure), then the planner's preferred version switch when it
 * breaks nothing. Never throws for a failed step: a step whose call fails, or
 * whose bridge call throws, is not applied — its reason is kept, with the mod
 * it was for — and the rest still run.
 */
export async function fixAll(instanceId: string, report: PreflightReport): Promise<FixAllResult> {
  const run: Run = { attempted: [], applied: [], failures: [] };
  // A self-completing pack still fetching its own files: its rows are advisory
  // (plan A17) and the pack brings those files itself — installing them here
  // would put a second jar beside the one it fetches.
  if (!hasBlocking(report)) return { attempted: [], applied: [], reasons: [] };
  const rows = (action: ViolationAction) =>
    report.violations.filter((v) => violationAction(v) === action);
  await enableDisabled(instanceId, rows('enable'), run);
  await installMissing(instanceId, rows('install'), run);
  await applyPlans(instanceId, rows('plan'), run);
  // Two rows of one mod can fail for one reason: it is named once.
  const once = new Map(run.failures.map((f) => [`${f.name}\u0000${f.reason}`, f]));
  return {
    attempted: run.attempted,
    applied: run.applied,
    reasons: reasonLines([...once.values()]),
  };
}

/** A row's dependency by the one naming rule the panel and the gate use (`depDisplayName`). */
const depName = (instanceId: string, v: DepViolation): string =>
  depDisplayName(v, depNameOf(instanceId, v.dependent_sha1, v.dep_id));

/** Record a failed step under `name`. */
function failWith(run: Run, name: string): (reason: string) => void {
  return (reason) => {
    run.failures.push({ name, reason });
  };
}

async function enableDisabled(instanceId: string, rows: DepViolation[], run: Run) {
  if (rows.length === 0) return;
  // One call for every jar the rows name (plan A5): the impact adds the disabled
  // mods they need and orders the flip; a jar several rows name goes on once,
  // named as the first row names it.
  const providers = new Map<string, string>();
  for (const v of rows) {
    if (v.provider_sha1 && !providers.has(v.provider_sha1))
      providers.set(v.provider_sha1, depName(instanceId, v));
  }
  const { enabled, failed } = await enableModsUnguarded(
    instanceId,
    [...providers].map(([sha1, name]) => ({ sha1, name })),
  );
  run.failures.push(...failed.map((f) => ({ name: f.name, reason: f.reason })));
  const on = new Set(enabled);
  for (const v of rows) {
    run.attempted.push(v);
    if (v.provider_sha1 && on.has(v.provider_sha1)) run.applied.push(v);
  }
}

async function installMissing(instanceId: string, rows: DepViolation[], run: Run) {
  // One install per mod-id: several dependents missing `balm` need ONE Balm — a
  // second jar providing the same id stops the game.
  const byDepId = new Map<string, boolean>();
  for (const v of rows) {
    run.attempted.push(v);
    let installed = byDepId.get(v.dep_id);
    if (installed === undefined) {
      // A failed install is named by the dependency it was to bring in.
      const fail = failWith(run, depName(instanceId, v));
      installed = await attempt(() => installDependency(instanceId, v, fail), false, fail);
      byDepId.set(v.dep_id, installed);
    }
    if (installed) run.applied.push(v);
  }
}

async function installDependency(
  instanceId: string,
  v: DepViolation,
  fail: (reason: string) => void,
): Promise<boolean> {
  // The project, when this pair's name was resolved earlier — by the Installed
  // tab, or by the Play gate as it opened; nothing here waits on the network
  // before Play. Otherwise the bare mod-id, which the backend resolves through
  // the dependent's own platform metadata.
  const project = depProjectOf(instanceId, v.dependent_sha1, v.dep_id);
  if (project) {
    const r = await commands.modsInstallDependency(
      instanceId,
      v.dependent_sha1,
      project.source,
      project.project_id,
    );
    return r.status === 'ok' || settled(r.error, fail);
  }
  const r = await commands.modsInstallMissingRequired(instanceId, v.dependent_sha1, v.dep_id);
  if (r.status === 'error') return settled(r.error, fail);
  if (r.data.kind === 'installed') return true;
  // `open_search`: the backend could not tell which project provides the id, so
  // it installed nothing.
  fail(get(t)('mods.preflight.notFound'));
  return false;
}

/**
 * A failed install call. The profile already listing that project is no
 * failure — an earlier step of this repair brought it under another mod-id, or
 * the user did meanwhile: there is nothing to install, and a second copy would
 * be the real fault; whether it satisfies the row is the re-run pre-flight's
 * call, like every step's. Anything else failed, and says why.
 */
function settled(e: IpcError, fail: (reason: string) => void): boolean {
  if (e.kind === 'mods_already_installed') return true;
  fail(modWriteReason(e));
  return false;
}

async function applyPlans(instanceId: string, rows: DepViolation[], run: Run) {
  // Jars this repair has tried to switch. A later row naming one was read
  // before the switch — the planner would judge a jar that may be gone (a failed
  // switch can still have removed it) — so it is left to the re-run pre-flight,
  // which judges what is there now.
  const touched = new Set<string>();
  for (const v of rows) {
    run.attempted.push(v);
    if (touched.has(v.dependent_sha1) || (v.provider_sha1 && touched.has(v.provider_sha1)))
      continue;
    // Until a switch is chosen the fix is the dependent's — the mod the panel's row names; a
    // chosen switch that fails is named by the mod it was to switch.
    const failRow = failWith(run, v.dependent_name);
    const switched = await attempt(
      async () => {
        const plan = await preferredSwitch(instanceId, v);
        if (plan.kind === 'failed') failRow(plan.reason);
        if (plan.kind !== 'switch') return false;
        touched.add(plan.oldSha1);
        // Named like every other update: after the build it becomes.
        const r = await updateMod(instanceId, plan.version.name, plan.oldSha1, plan.version);
        if (r.status === 'ok') return true;
        const switching =
          plan.oldSha1 === v.dependent_sha1 ? v.dependent_name : depName(instanceId, v);
        failWith(run, switching)(modWriteReason(r.error));
        return false;
      },
      false,
      failRow,
    );
    if (switched) run.applied.push(v);
  }
}

type PreferredSwitch =
  | { kind: 'switch'; oldSha1: string; version: ModVersion }
  /** Nothing this repair may take: no build either side, or only ones that break another mod. */
  | { kind: 'none' }
  /** The planner could not ask the platform — never "no version". */
  | { kind: 'failed'; reason: string };

/**
 * The planner's preferred switch for `v`, if it offers one this repair may take — the offer the
 * panel makes its default (`planOffers`). Never a change that breaks another mod without an
 * explicit click (D8), on either side: updating the dependent can push it out of a range another
 * mod declares on it. So the dependent's update when it breaks nothing — it leaves the dependency
 * alone — else the dependency's change when that breaks nothing, else nothing.
 */
async function preferredSwitch(instanceId: string, v: DepViolation): Promise<PreferredSwitch> {
  const plan = await commands.modsPlanVersionFix(instanceId, v.dependent_sha1, v.dep_id);
  if (plan.status !== 'ok') return { kind: 'failed', reason: modWriteReason(plan.error) };
  const { update_dependent: dependent, change_provider: provider } = plan.data;
  if (dependent && dependent.breaks.length === 0) {
    return { kind: 'switch', oldSha1: v.dependent_sha1, version: dependent.version };
  }
  // Never beside a provider the registry does not track — that would install a second jar.
  if (provider && provider.breaks.length === 0 && v.provider_sha1) {
    return { kind: 'switch', oldSha1: v.provider_sha1, version: provider.version };
  }
  return { kind: 'none' };
}

/**
 * One repair step. A throw is a bridge failure with no Result to read: the step
 * is not applied — never read as success — its message is a reason like any
 * failure's, recorded by `fail` under the mod the step was for, and the repair
 * goes on; the re-run pre-flight reports what is left either way.
 */
async function attempt<T>(
  step: () => Promise<T>,
  failed: T,
  fail: (reason: string) => void,
): Promise<T> {
  try {
    return await step();
  } catch (e) {
    console.warn('[fix-all] a repair step failed:', e);
    fail(e instanceof Error ? e.message : String(e));
    return failed;
  }
}

/**
 * How many of `attempted` the re-check no longer reports. A row is still there
 * when the re-check names the same dependency for the same dependent — by its
 * jar, or by its name: switching a mod to another build changes its jar (and
 * digest), not a problem the new build still has.
 */
export function countFixed(attempted: readonly DepViolation[], after: PreflightReport): number {
  const byJar = new Set(after.violations.map(violationKey));
  const byName = new Set(after.violations.map(nameKey));
  return attempted.filter((v) => !byJar.has(violationKey(v)) && !byName.has(nameKey(v))).length;
}

const nameKey = (v: DepViolation): string => `${v.dependent_name}\u0000${v.dep_id}`;

export type RepairOutcome =
  | { kind: 'launch'; checked: boolean }
  | { kind: 'stay'; report: PreflightReport; fixed: number; total: number; reasons: string[] };

/**
 * The Play gate's loop (spec D5): repair, re-run the pre-flight, decide. Clean →
 * launch. Still blocking → stay on the rows that remain, with «Fixed N of M» and
 * why the steps that failed failed. A re-check that cannot run launches like
 * any unchecked Play (maintainer rule: a failed check never blocks) —
 * `checked: false`, so the page can say so.
 */
export async function repairForLaunch(
  instanceId: string,
  report: PreflightReport,
  recheck: () => Promise<PreflightResult>,
): Promise<RepairOutcome> {
  const { attempted, reasons } = await fixAll(instanceId, report);
  const decision = decideLaunch(await recheck());
  if (decision.kind !== 'gate') return { kind: 'launch', checked: decision.kind === 'launch' };
  return {
    kind: 'stay',
    report: decision.report,
    fixed: countFixed(attempted, decision.report),
    total: attempted.length,
    reasons,
  };
}
