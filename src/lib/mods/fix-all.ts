import { get } from 'svelte/store';
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
import { type ViolationAction, violationAction } from './violation-view';

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
   * Why the steps that failed failed, each reason once, worded for the user — a
   * busy profile as busy (`modWriteReason`). «Fixed 0 of 3» alone cannot tell a
   * held profile from three unrelated failures. A row the repair had no fix for
   * (no build breaks nothing, an unidentified jar) is no failed step and gives
   * none: the row that stays says what is wrong.
   */
  reasons: string[];
};

type PreflightResult = Parameters<typeof decideLaunch>[0];

/**
 * Apply every automatic repair in `report`: switch disabled providers on (with
 * the disabled mods they need), install missing dependencies (each with its
 * own required closure), then the planner's preferred version switch when it
 * breaks nothing. Never throws for a failed step: a step whose call fails, or
 * whose bridge call throws, is not applied — its reason is kept — and the rest
 * still run.
 */
export async function fixAll(instanceId: string, report: PreflightReport): Promise<FixAllResult> {
  const out: FixAllResult = { attempted: [], applied: [], reasons: [] };
  // A self-completing pack still fetching its own files: its rows are advisory
  // (plan A17) and the pack brings those files itself — installing them here
  // would put a second jar beside the one it fetches.
  if (!hasBlocking(report)) return out;
  const rows = (action: ViolationAction) =>
    report.violations.filter((v) => violationAction(v) === action);
  await enableDisabled(instanceId, rows('enable'), out);
  await installMissing(instanceId, rows('install'), out);
  await applyPlans(instanceId, rows('plan'), out);
  return { ...out, reasons: [...new Set(out.reasons)] };
}

async function enableDisabled(instanceId: string, rows: DepViolation[], out: FixAllResult) {
  if (rows.length === 0) return;
  // One call for every jar the rows name (plan A5): the impact adds the disabled
  // mods they need and orders the flip; a jar several rows name goes on once.
  const providers = [...new Set(rows.flatMap((v) => (v.provider_sha1 ? [v.provider_sha1] : [])))];
  const { enabled, reasons } = await enableModsUnguarded(instanceId, providers);
  out.reasons.push(...reasons);
  const on = new Set(enabled);
  for (const v of rows) {
    out.attempted.push(v);
    if (v.provider_sha1 && on.has(v.provider_sha1)) out.applied.push(v);
  }
}

async function installMissing(instanceId: string, rows: DepViolation[], out: FixAllResult) {
  // One install per mod-id: several dependents missing `balm` need ONE Balm — a
  // second jar providing the same id stops the game.
  const byDepId = new Map<string, boolean>();
  for (const v of rows) {
    out.attempted.push(v);
    let installed = byDepId.get(v.dep_id);
    if (installed === undefined) {
      installed = await attempt(() => installDependency(instanceId, v, out.reasons), false, out);
      byDepId.set(v.dep_id, installed);
    }
    if (installed) out.applied.push(v);
  }
}

async function installDependency(
  instanceId: string,
  v: DepViolation,
  reasons: string[],
): Promise<boolean> {
  // The project, when the Installed tab resolved this pair's name earlier —
  // nothing here asks the network before Play. Otherwise the bare mod-id, which
  // the backend resolves through the dependent's own platform metadata.
  const project = depProjectOf(instanceId, v.dependent_sha1, v.dep_id);
  if (project) {
    const r = await commands.modsInstallDependency(
      instanceId,
      v.dependent_sha1,
      project.source,
      project.project_id,
    );
    return r.status === 'ok' || settled(r.error, reasons);
  }
  const r = await commands.modsInstallMissingRequired(instanceId, v.dependent_sha1, v.dep_id);
  if (r.status === 'error') return settled(r.error, reasons);
  if (r.data.kind === 'installed') return true;
  // `open_search`: the backend could not tell which project provides the id, so
  // it installed nothing.
  const dep = depNameOf(instanceId, v.dependent_sha1, v.dep_id) ?? v.dep_id;
  reasons.push(get(t)('mods.preflight.notFound', { dep }));
  return false;
}

/**
 * A failed install call. The profile already listing that project is no
 * failure — an earlier step of this repair brought it under another mod-id, or
 * the user did meanwhile: there is nothing to install, and a second copy would
 * be the real fault; whether it satisfies the row is the re-run pre-flight's
 * call, like every step's. Anything else failed, and says why.
 */
function settled(e: IpcError, reasons: string[]): boolean {
  if (e.kind === 'mods_already_installed') return true;
  reasons.push(modWriteReason(e));
  return false;
}

async function applyPlans(instanceId: string, rows: DepViolation[], out: FixAllResult) {
  // Jars this repair has tried to switch. A later row naming one was read
  // before the switch — the planner would judge a jar that may be gone (a failed
  // switch can still have removed it) — so it is left to the re-run pre-flight,
  // which judges what is there now.
  const touched = new Set<string>();
  for (const v of rows) {
    out.attempted.push(v);
    if (touched.has(v.dependent_sha1) || (v.provider_sha1 && touched.has(v.provider_sha1)))
      continue;
    const switched = await attempt(
      async () => {
        const plan = await preferredSwitch(instanceId, v);
        if (plan.kind === 'failed') out.reasons.push(plan.reason);
        if (plan.kind !== 'switch') return false;
        touched.add(plan.oldSha1);
        // Named like every other update: after the build it becomes.
        const r = await updateMod(instanceId, plan.version.name, plan.oldSha1, plan.version);
        if (r.status === 'ok') return true;
        out.reasons.push(modWriteReason(r.error));
        return false;
      },
      false,
      out,
    );
    if (switched) out.applied.push(v);
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
 * failure's, and the repair goes on; the re-run pre-flight reports what is left
 * either way.
 */
async function attempt<T>(step: () => Promise<T>, failed: T, out: FixAllResult): Promise<T> {
  try {
    return await step();
  } catch (e) {
    console.warn('[fix-all] a repair step failed:', e);
    out.reasons.push(e instanceof Error ? e.message : String(e));
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
