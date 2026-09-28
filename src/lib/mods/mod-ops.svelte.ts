/**
 * Guarded mod operations — the ONE path every enable / disable / uninstall of an instance mod
 * takes: the Installed row, the bulk bar, the Browse card and the imported-pack drawer (spec
 * 2026-09-28 §6.1). Mods only; assets keep their direct commands.
 *
 * - Disable / uninstall ask `mods_removal_impact` first, enable asks `mods_enable_impact` — each
 *   ONE call for all targets. Nothing affected → act at once. Otherwise the question goes to the
 *   app-level ModOpsHost (DESIGN.md §8, app-level blocking dialogs) and the returned promise
 *   settles with the user's choice AND the mutation's outcome, while the dialog shows which
 *   button runs.
 * - A check that could not run is never read as "nothing depends on it" (fallback Q1/Q2): the
 *   dialog says the check failed and asks.
 * - A flip that carries its dependents or requirements along keeps every step launchable: what
 *   needs a target is switched off before it, what a target needs is switched on before it, and
 *   the first failure ends the run.
 * - Uninstall ends with an Undo toast (10 s, paused on hover/focus) that restores the batch from
 *   the per-instance trash by its token; once everything is back, the dependents the same removal
 *   disabled are switched on again (plan A7). Toasts are module-level, so Undo outlives tab and
 *   profile switches; every flow captures its instance id at call time.
 * - A refusal closes the dialog and says why; nothing is half-done. A mod write takes the SHARED
 *   maintenance claim, so `instance_busy` here means a long operation (a pack update, a migration,
 *   a clone) holds the profile — never that the game runs (plan A9).
 */
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import {
  commands,
  type DisabledRequirement,
  type ImpactedMod,
  type Error as IpcError,
  type OrphanRef,
  type RestoreSkipReason,
  type UninstallReceipt,
} from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { pushActionToast, pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';

export type ModOpTarget = { sha1: string; name: string };
export type ModOpScope = {
  instanceId: string;
  /** Named in a restore toast when the user has switched to another profile. */
  profileName?: string | null;
  /** The caller's display name for any sha1 of this instance (`rowDisplayName`). */
  nameOf?: (sha1: string) => string | null | undefined;
};
export type ModOpOutcome = 'applied' | 'cancelled' | 'failed';
export type ImpactChoice = 'primary' | 'secondary' | 'cancel';

export type ImpactView =
  | { mode: 'dependents-on-disable'; targets: ModOpTarget[]; dependents: ImpactedMod[] }
  | { mode: 'dependents-on-remove'; targets: ModOpTarget[]; dependents: ImpactedMod[] }
  | {
      mode: 'enable-with-requirements';
      targets: ModOpTarget[];
      requirements: DisabledRequirement[];
    }
  | {
      mode: 'impact-check-failed';
      action: 'disable' | 'uninstall' | 'enable';
      targets: ModOpTarget[];
      error: string;
    };
export type OrphansView = { mode: 'orphans'; targets: ModOpTarget[]; orphans: OrphanRef[] };
export type OpsView = ImpactView | OrphansView;
/** Which button of the open dialog runs its mutation; null while it waits for an answer. */
export type OpsBusy = 'primary' | 'secondary' | null;
export type OpsDialog = {
  readonly id: number;
  readonly view: OpsView | null;
  readonly busy: OpsBusy;
};
type Answer = { choice: ImpactChoice; alsoRemove: string[] };

/** How long an Undo toast stays (spec D2); its countdown pauses on hover/focus. */
export const UNDO_TTL_MS = 10_000;
/** A bulk undo toast lists this many names, then «and N more». */
const LIST_CAP = 5;
const SKIP_KEY: Record<RestoreSkipReason, TranslationKey> = {
  name_taken: 'mods.ops.restore.nameTaken',
  missing: 'mods.ops.restore.missing',
  already_installed: 'mods.ops.restore.alreadyInstalled',
};

let dialogId = $state(0);
// Replaced whole, never mutated: no deep proxy.
let dialogView = $state.raw<OpsView | null>(null);
let dialogBusy = $state<OpsBusy>(null);
let pending: { id: number; resolve: (a: Answer) => void } | null = null;
let seq = 0;
let activeInstanceId: string | null = null;

const reader: OpsDialog = {
  get id() {
    return dialogId;
  },
  get view() {
    return dialogView;
  },
  get busy() {
    return dialogBusy;
  },
};

/** The open question (reactive getters), read by ModOpsHost. */
export function opsDialog(): OpsDialog {
  return reader;
}

/** ModOpsHost publishes the active profile so a restore can say where it landed. */
export function setOpsActiveInstance(id: string | null): void {
  activeInstanceId = id;
}

/** ModOpsHost → the flow waiting on the open dialog. Ignored while the chosen mutation runs. */
export function answerDialog(choice: ImpactChoice, alsoRemove: readonly string[] = []): void {
  if (dialogBusy !== null) return;
  settle({ choice, alsoRemove: [...alsoRemove] });
}

export function __resetModOpsForTests(): void {
  settle({ choice: 'cancel', alsoRemove: [] });
  close(dialogId);
  activeInstanceId = null;
}

function ask(view: OpsView): { id: number; answer: Promise<Answer> } {
  // A newer question supersedes an unanswered one: the older flow settles as cancelled instead
  // of waiting on a dialog nobody can see any more.
  settle({ choice: 'cancel', alsoRemove: [] });
  seq += 1;
  const id = seq;
  dialogId = id;
  dialogView = view;
  dialogBusy = null;
  const answer = new Promise<Answer>((resolve) => {
    pending = { id, resolve };
  });
  return { id, answer };
}

function settle(a: Answer): void {
  const p = pending;
  if (!p) return;
  pending = null;
  // The orphan question has no busy state and closes at once; an impact dialog stays open with
  // the chosen button spinning until its mutation ends (`finish`).
  if (a.choice === 'cancel' || dialogView?.mode === 'orphans') close(p.id);
  else dialogBusy = a.choice;
  p.resolve(a);
}

function close(id: number): void {
  if (dialogId !== id) return;
  dialogView = null;
  dialogBusy = null;
}

/** Close dialog `id` and report `outcome` — for an answer that runs nothing. */
function closeAs(id: number, outcome: ModOpOutcome): ModOpOutcome {
  close(id);
  return outcome;
}

async function finish(id: number, run: Promise<ModOpOutcome>): Promise<ModOpOutcome> {
  try {
    return await run;
  } finally {
    close(id);
  }
}

type Settled<T> = { ok: true; data: T } | { ok: false; message: string; busy: boolean };

async function settleCall<T>(
  run: () => Promise<{ status: 'ok'; data: T } | { status: 'error'; error: IpcError }>,
): Promise<Settled<T>> {
  try {
    const r = await run();
    if (r.status === 'ok') return { ok: true, data: r.data };
    return { ok: false, message: reasonOf(r.error), busy: r.error.kind === 'instance_busy' };
  } catch (e) {
    // The bridge itself failed (e.g. torn down mid-call): no Result to read. Reported as a
    // failure — never read as success, never as "nothing depends on it".
    return { ok: false, message: e instanceof Error ? e.message : String(e), busy: false };
  }
}

// The shared `instance_busy` copy also names a running game, which is true for a launch or a
// long writer but false for a mod write: the shared claim admits writers while the game runs.
function reasonOf(e: IpcError): string {
  return e.kind === 'instance_busy' ? tr()('mods.ops.busy') : formatError(e);
}

const tr = () => get(t);
const shas = (xs: readonly { sha1: string }[]) => xs.map((x) => x.sha1);
const asTarget = (x: { sha1: string; name: string }): ModOpTarget => ({
  sha1: x.sha1,
  name: x.name,
});
const named = <T extends { sha1: string; name: string }>(scope: ModOpScope, x: T): T => ({
  ...x,
  name: scope.nameOf?.(x.sha1) ?? x.name,
});

function listLines(names: readonly string[]): string[] {
  if (names.length <= LIST_CAP + 1) return [...names];
  return [
    ...names.slice(0, LIST_CAP),
    tr()('mods.ops.undo.more', { count: names.length - LIST_CAP }),
  ];
}

function profileLine(scope: ModOpScope): string[] {
  if (scope.instanceId === activeInstanceId || !scope.profileName) return [];
  return [tr()('mods.ops.restore.inProfile', { profile: scope.profileName })];
}

type FlipResult = { done: ModOpTarget[]; failed: { target: ModOpTarget; message: string }[] };

/**
 * Enable or disable `targets` one by one, in order. `stopOnFailure`: each step relies on the
 * ones before it (a requirement before what needs it, a dependent before what it needs), so the
 * first failure ends the run and the steps after it count as failed for the same reason —
 * untried. Otherwise every target gets its try.
 */
async function flipAll(
  instanceId: string,
  targets: readonly ModOpTarget[],
  enabled: boolean,
  stopOnFailure = false,
): Promise<FlipResult> {
  const done: ModOpTarget[] = [];
  const failed: FlipResult['failed'] = [];
  for (const [i, target] of targets.entries()) {
    const r = await settleCall(() =>
      enabled
        ? commands.modsEnable(instanceId, target.sha1)
        : commands.modsDisable(instanceId, target.sha1),
    );
    if (r.ok) {
      done.push(target);
      continue;
    }
    failed.push({ target, message: r.message });
    if (stopOnFailure) {
      for (const rest of targets.slice(i + 1)) failed.push({ target: rest, message: r.message });
      break;
    }
  }
  return { done, failed };
}

async function applyEnabled(
  scope: ModOpScope,
  flip: readonly ModOpTarget[],
  enabled: boolean,
  bulk: boolean,
  stopOnFailure = false,
): Promise<ModOpOutcome> {
  const r = await flipAll(scope.instanceId, flip, enabled, stopOnFailure);
  const tt = tr();
  const where = profileLine(scope);
  if (r.failed.length === 0) {
    // A single row flips in place; a batch says what it did.
    if (bulk || flip.length > 1)
      pushSuccess(
        tt(enabled ? 'mods.installed.toastEnabled' : 'mods.installed.toastDisabled', {
          count: r.done.length,
        }),
        where,
      );
    return 'applied';
  }
  // The distinct reasons, once each: "3 failed" alone cannot tell a held profile (every row
  // refused the same way) from three unrelated failures.
  const reasons = [...new Set(r.failed.map((f) => f.message))];
  const one = flip.length === 1 && !bulk ? flip[0] : null;
  const title = one
    ? tt(enabled ? 'mods.ops.failed.enable' : 'mods.ops.failed.disable', { name: one.name })
    : tt(enabled ? 'mods.installed.toastEnabledFailed' : 'mods.installed.toastDisabledFailed', {
        count: r.done.length,
        failed: r.failed.length,
      });
  pushWarning(title, [...reasons, ...where]);
  return r.done.length > 0 ? 'applied' : 'failed';
}

function checkFailed(
  action: 'disable' | 'uninstall' | 'enable',
  targets: readonly ModOpTarget[],
  error: string,
): { id: number; answer: Promise<Answer> } {
  return ask({ mode: 'impact-check-failed', action, targets: [...targets], error });
}

/** Disable `targets` in one instance, asking first when other enabled mods need them. */
export async function disableMods(
  scope: ModOpScope,
  targets: readonly ModOpTarget[],
  opts: { bulk?: boolean } = {},
): Promise<ModOpOutcome> {
  if (targets.length === 0) return 'cancelled';
  const bulk = opts.bulk === true;
  const impact = await settleCall(() =>
    commands.modsRemovalImpact(scope.instanceId, shas(targets)),
  );
  if (!impact.ok) {
    const q = checkFailed('disable', targets, impact.message);
    if ((await q.answer).choice !== 'primary') return closeAs(q.id, 'cancelled');
    return finish(q.id, applyEnabled(scope, targets, false, bulk));
  }
  const dependents = impact.data.dependents.map((d) => named(scope, d));
  if (dependents.length === 0) return applyEnabled(scope, targets, false, bulk);
  const q = ask({ mode: 'dependents-on-disable', targets: [...targets], dependents });
  const { choice } = await q.answer;
  if (choice === 'cancel') return 'cancelled';
  if (choice === 'secondary') return finish(q.id, applyEnabled(scope, targets, false, bulk));
  // Dependents first: a target switched off while a mod that needs it stayed on would not load.
  const flip = [...dependents.map(asTarget), ...targets];
  return finish(q.id, applyEnabled(scope, flip, false, bulk, true));
}

/** Enable `targets`, asking first when they need mods that are disabled. */
export async function enableMods(
  scope: ModOpScope,
  targets: readonly ModOpTarget[],
  opts: { bulk?: boolean } = {},
): Promise<ModOpOutcome> {
  if (targets.length === 0) return 'cancelled';
  const bulk = opts.bulk === true;
  // One call for all targets (plan A5): the backend unions what they need, transitively, minus
  // the targets themselves.
  const impact = await settleCall(() => commands.modsEnableImpact(scope.instanceId, shas(targets)));
  if (!impact.ok) {
    const q = checkFailed('enable', targets, impact.message);
    if ((await q.answer).choice !== 'primary') return closeAs(q.id, 'cancelled');
    return finish(q.id, applyEnabled(scope, targets, true, bulk));
  }
  const requirements = impact.data.requirements.map((r) => named(scope, r));
  if (requirements.length === 0) return applyEnabled(scope, targets, true, bulk);
  const q = ask({ mode: 'enable-with-requirements', targets: [...targets], requirements });
  const { choice } = await q.answer;
  if (choice === 'cancel') return 'cancelled';
  if (choice === 'secondary') return finish(q.id, applyEnabled(scope, targets, true, bulk));
  // Requirements first, deepest first: the backend lists them in the order it found them, a
  // requirement's own requirement after it. So nothing comes up without what it needs.
  const flip = [...requirements.map(asTarget).reverse(), ...targets];
  return finish(q.id, applyEnabled(scope, flip, true, bulk, true));
}

// The optional "also remove unneeded libraries" question (bulk). A failed lookup offers nothing:
// the restrictive direction — nothing extra is removed, which is what "no" would have done — and
// the removal the user asked for goes ahead.
async function withOrphans(
  scope: ModOpScope,
  targets: readonly ModOpTarget[],
): Promise<ModOpTarget[] | null> {
  const r = await settleCall(() => commands.modsFindOrphans(scope.instanceId, shas(targets)));
  const orphans = r.ok ? r.data : [];
  if (orphans.length === 0) return [...targets];
  const { choice, alsoRemove } = await ask({ mode: 'orphans', targets: [...targets], orphans })
    .answer;
  if (choice === 'cancel') return null;
  const picked = orphans
    .filter((o) => alsoRemove.includes(o.sha1))
    .map((o) => named(scope, { sha1: o.sha1, name: o.name }));
  return [...targets, ...picked];
}

/** Remove `targets` into the trash (one token), then offer Undo. */
export async function uninstallMods(
  scope: ModOpScope,
  targets: readonly ModOpTarget[],
  opts: { offerOrphans?: boolean } = {},
): Promise<ModOpOutcome> {
  if (targets.length === 0) return 'cancelled';
  const impact = await settleCall(() =>
    commands.modsRemovalImpact(scope.instanceId, shas(targets)),
  );
  let dialog: number | null = null;
  let alsoDisable: ModOpTarget[] = [];
  if (!impact.ok) {
    const q = checkFailed('uninstall', targets, impact.message);
    if ((await q.answer).choice !== 'primary') return closeAs(q.id, 'cancelled');
    dialog = q.id;
  } else if (impact.data.dependents.length > 0) {
    const dependents = impact.data.dependents.map((d) => named(scope, d));
    const q = ask({ mode: 'dependents-on-remove', targets: [...targets], dependents });
    const { choice } = await q.answer;
    if (choice === 'cancel') return 'cancelled';
    dialog = q.id;
    // Dependents are disabled, never removed (D3); «Only this one» leaves them enabled.
    if (choice === 'primary') alsoDisable = dependents.map(asTarget);
  }
  try {
    const removing = opts.offerOrphans ? await withOrphans(scope, targets) : [...targets];
    if (removing === null) return 'cancelled';
    const gone = new Set(shas(removing));
    return await removeNow(
      scope,
      removing,
      alsoDisable.filter((d) => !gone.has(d.sha1)),
    );
  } finally {
    if (dialog !== null) close(dialog);
  }
}

async function removeNow(
  scope: ModOpScope,
  removing: readonly ModOpTarget[],
  alsoDisable: readonly ModOpTarget[],
): Promise<ModOpOutcome> {
  const tt = tr();
  const only = removing.length === 1 ? removing[0] : null;
  const r = await settleCall<UninstallReceipt>(() =>
    only
      ? commands.modsUninstall(scope.instanceId, only.sha1)
      : commands.modsUninstallMany(scope.instanceId, shas(removing)),
  );
  if (!r.ok) {
    // Refused (InstanceBusy) or failed: the backend moved nothing (it puts a partial move back),
    // so there is nothing to undo — say why and stop.
    pushWarning(
      only
        ? tt('mods.ops.failed.removeOne', { name: only.name })
        : tt('mods.ops.failed.removeMany', { count: removing.length }),
      [r.message, ...profileLine(scope)],
    );
    return 'failed';
  }
  // Only after the removal happened: a refused removal must not leave its dependents disabled.
  // Each dependent is independent of the others, so every one gets its try.
  const disabled = await flipAll(scope.instanceId, alsoDisable, false);
  pushUndo(scope, r.data, removing, disabled);
  return 'applied';
}

function pushUndo(
  scope: ModOpScope,
  receipt: UninstallReceipt,
  removing: readonly ModOpTarget[],
  disabled: FlipResult,
): void {
  const tt = tr();
  const nameBy = new Map(removing.map((x) => [x.sha1, x.name]));
  const shown = receipt.items.map((i) => nameBy.get(i.sha1) ?? i.name);
  // No items: every digest was already gone, nothing moved — nothing to undo (plan §5 item 15).
  if (shown.length === 0) return;
  const title =
    shown.length === 1
      ? tt('mods.ops.undo.removedOne', { name: shown[0] })
      : tt('mods.ops.undo.removedMany', { count: shown.length });
  const lines = [
    ...(shown.length > 1 ? listLines(shown) : []),
    ...(disabled.done.length > 0
      ? [tt('mods.ops.undo.alsoDisabled', { names: disabled.done.map((d) => d.name).join(', ') })]
      : []),
    ...disabled.failed.map(
      (f) => `${tt('mods.ops.failed.disable', { name: f.target.name })}: ${f.message}`,
    ),
    ...profileLine(scope),
  ];
  const run = () => void restoreUninstalled(scope, receipt.token, shown, disabled.done);
  pushActionToast(
    disabled.failed.length > 0 ? 'warning' : 'success',
    title,
    { label: tt('mods.ops.undo.action'), run },
    lines,
    { ttlMs: UNDO_TTL_MS },
  );
}

async function restoreUninstalled(
  scope: ModOpScope,
  token: string,
  shown: readonly string[],
  reEnable: readonly ModOpTarget[],
): Promise<void> {
  const tt = tr();
  const r = await settleCall(() => commands.modsRestoreUninstalled(scope.instanceId, token));
  // Read after the call: the user may have switched profiles meanwhile.
  const where = profileLine(scope);
  const stillDisabled =
    reEnable.length > 0
      ? [tt('mods.ops.restore.stillDisabled', { names: reEnable.map((d) => d.name).join(', ') })]
      : [];
  if (!r.ok) {
    if (r.busy) {
      // Refused while a long operation holds the profile; the token stays valid (§6.1, §9), so
      // the undo comes back as a sticky toast that tries again.
      const what =
        shown.length === 1 ? shown[0] : tt('mods.ops.restore.modsCount', { count: shown.length });
      const retry = () => void restoreUninstalled(scope, token, shown, reEnable);
      pushActionToast(
        'warning',
        tt('mods.ops.restore.busy', { what }),
        { label: tt('mods.ops.undo.action'), run: retry },
        where,
      );
      return;
    }
    // The backend stops at the first item it cannot put back, and the ones before it ARE back
    // (their `mod-installed` events refresh the views) — so never "nothing came back".
    pushWarning(tt('mods.ops.restore.incomplete'), [r.message, ...stillDisabled, ...where]);
    return;
  }
  const report = r.data;
  if (report.expired) {
    pushWarning(tt('mods.ops.restore.expired'), [...stillDisabled, ...where]);
    return;
  }
  const skipped = report.skipped.map((s) => tt(SKIP_KEY[s.reason], { name: s.name }));
  let dependentLines: string[] = [];
  if (reEnable.length > 0 && skipped.length === 0) {
    const back = await flipAll(scope.instanceId, reEnable, true);
    dependentLines = back.failed.map(
      (f) => `${tt('mods.ops.restore.reenableFailed', { name: f.target.name })}: ${f.message}`,
    );
  } else {
    // Something did not come back: switching on what needs it would only break the next launch.
    dependentLines = stillDisabled;
  }
  const restored = report.restored.length;
  const title =
    restored > 0 ? tt('mods.ops.restore.done', { count: restored }) : tt('mods.ops.restore.failed');
  if (restored > 0 && skipped.length === 0 && dependentLines.length === 0)
    pushSuccess(title, where);
  else pushWarning(title, [...skipped, ...dependentLines, ...where]);
}
