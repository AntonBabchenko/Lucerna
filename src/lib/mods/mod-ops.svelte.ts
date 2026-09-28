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
 * - Every flip keeps each step launchable. The impact answer carries ONE safe flip `order` over
 *   the targets and the mods they carry along — switching off, a mod before any of them it needs;
 *   switching on, after — because targets may need each other, and a need can run either way
 *   between a target and a mod it carries along. The flip follows it exactly as given, filtered
 *   to the targets for «Only this one», and the first failure ends the run: the steps after it
 *   count as failed for the same reason. Only when the check could not run is there no order:
 *   then every target gets its try.
 * - One question at a time. A newer question supersedes an unanswered one (that flow settles as
 *   cancelled), but never a dialog whose chosen mutation still runs: the question waits until
 *   that dialog closes. Once the last ModOpsHost is gone, every flow still waiting for an answer
 *   settles as cancelled.
 * - Uninstall ends with an Undo toast (10 s, paused on hover/focus) that restores the batch from
 *   the per-instance trash by its token; once everything is back, the dependents the same removal
 *   disabled are switched on again (plan A7) — in reverse of the order they went off, providers
 *   first, stopping at the first failure. Toasts are module-level, so Undo outlives tab and
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

/** A question put to the user, with the way to settle the flow that asked it. */
type Question = { id: number; view: OpsView; resolve: (a: Answer) => void };
/** What ModOpsHost renders: the open question, and which of its buttons runs. */
type Shown = { readonly id: number; readonly view: OpsView | null; readonly busy: OpsBusy };

// Every decision reads PLAIN module state, so one taken while the host is being torn down — when
// Svelte serves pre-batch `$state` values — still sees what is current. `shown` is its reactive
// copy for ModOpsHost: replaced whole by `publish`, never read here.
let current: Shown = { id: 0, view: null, busy: null };
let shown = $state.raw<Shown>(current);
/** The flow waiting for an answer to the question on screen. */
let pending: { id: number; resolve: (a: Answer) => void } | null = null;
/** A question that arrived while another flow's mutation ran under its dialog. */
let waiting: Question | null = null;
let hosts = 0;
let seq = 0;
let activeInstanceId: string | null = null;

const reader: OpsDialog = {
  get id() {
    return shown.id;
  },
  get view() {
    return shown.view;
  },
  get busy() {
    return shown.busy;
  },
};

const cancelled = (): Answer => ({ choice: 'cancel', alsoRemove: [] });

function publish(next: Shown): void {
  current = next;
  shown = next;
}

/** The open question (reactive getters), read by ModOpsHost. */
export function opsDialog(): OpsDialog {
  return reader;
}

/** ModOpsHost publishes the active profile so a restore can say where it landed. */
export function setOpsActiveInstance(id: string | null): void {
  activeInstanceId = id;
}

/**
 * ModOpsHost registers while it is mounted; the returned function unregisters it. Once no host is
 * left nothing can answer a question, so every flow still waiting for one — on screen or queued —
 * settles as cancelled. A dialog whose mutation still runs is left to its flow, which closes it.
 */
export function attachOpsHost(): () => void {
  hosts += 1;
  let attached = true;
  return () => {
    if (!attached) return;
    attached = false;
    hosts -= 1;
    if (hosts > 0) return;
    dropWaiting();
    settle(cancelled());
  };
}

/** ModOpsHost → the flow waiting on the open dialog. Ignored while the chosen mutation runs. */
export function answerDialog(choice: ImpactChoice, alsoRemove: readonly string[] = []): void {
  if (current.busy !== null) return;
  settle({ choice, alsoRemove: [...alsoRemove] });
}

export function __resetModOpsForTests(): void {
  dropWaiting();
  settle(cancelled());
  close(current.id);
  activeInstanceId = null;
}

/**
 * Put `view` to the user; `answer` settles with the choice. A newer question supersedes an
 * unanswered one, shown or waiting: the older flow settles as cancelled instead of waiting on a
 * dialog nobody will see. A dialog whose chosen mutation still runs is never replaced — its
 * spinner is the only sign the operation is under way — so the question waits until that dialog
 * closes, then shows. `own` is the asking flow's own dialog: its follow-up question (the orphan
 * offer) takes that dialog's place, and a question waiting behind it keeps waiting.
 */
function ask(view: OpsView, own: number | null = null): { id: number; answer: Promise<Answer> } {
  seq += 1;
  const id = seq;
  const answer = new Promise<Answer>((resolve) => {
    const q: Question = { id, view, resolve };
    if (own !== null && current.id === own) {
      show(q);
      return;
    }
    dropWaiting();
    if (current.busy !== null) waiting = q;
    else show(q);
  });
  return { id, answer };
}

function show(q: Question): void {
  // The question on screen, if still unanswered, is superseded.
  settle(cancelled());
  pending = { id: q.id, resolve: q.resolve };
  publish({ id: q.id, view: q.view, busy: null });
}

/** The question waiting behind a running dialog is superseded or orphaned: cancel its flow. */
function dropWaiting(): void {
  const w = waiting;
  waiting = null;
  w?.resolve(cancelled());
}

function settle(a: Answer): void {
  const p = pending;
  if (!p) return;
  pending = null;
  // The orphan question has no busy state and closes at once; an impact dialog stays open with
  // the chosen button spinning until its mutation ends (`finish`).
  if (a.choice === 'cancel' || current.view?.mode === 'orphans') close(p.id);
  else publish({ ...current, busy: a.choice });
  p.resolve(a);
}

function close(id: number): void {
  if (current.id !== id || current.view === null) return;
  publish({ id, view: null, busy: null });
  // A question that waited for this dialog shows now.
  const next = waiting;
  waiting = null;
  if (next) show(next);
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

/**
 * `items` in the backend's safe flip `order` (sha1s) — any part of it is still safe for that
 * part. An item `order` does not name keeps its place after those it does: a mod asked for is
 * never dropped.
 */
function inOrder(order: readonly string[], items: readonly ModOpTarget[]): ModOpTarget[] {
  const at = new Map<string, number>(order.map((sha1, i) => [sha1, i]));
  const rank = (x: ModOpTarget) => at.get(x.sha1) ?? order.length;
  // `sort` is stable: items of equal rank keep the order they came in.
  return [...items].sort((a, b) => rank(a) - rank(b));
}

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
 * Enable or disable `targets` one by one, in order. `stopOnFailure`: they come in a safe flip
 * order, where each step relies on the ones before it (a mod comes on after what it needs, goes
 * off before it), so the first failure ends the run and the steps after it count as failed for
 * the same reason — untried. Otherwise every target gets its try.
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

/**
 * Flip `flip` and say what happened. `bulk`: a batch says what it did even when it is one mod.
 * `ordered`: `flip` follows the backend's safe order, so the first failure ends the run.
 */
async function applyEnabled(
  scope: ModOpScope,
  flip: readonly ModOpTarget[],
  enabled: boolean,
  { bulk, ordered }: { bulk: boolean; ordered: boolean },
): Promise<ModOpOutcome> {
  const r = await flipAll(scope.instanceId, flip, enabled, ordered);
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
    return finish(q.id, applyEnabled(scope, targets, false, { bulk, ordered: false }));
  }
  // In the backend's safe disable order — a mod goes off before any of them it needs — so
  // nothing goes off while a mod that needs it stays on; the targets too may need each other.
  const { order } = impact.data;
  const flipInOrder = (flip: readonly ModOpTarget[]) =>
    applyEnabled(scope, inOrder(order, flip), false, { bulk, ordered: true });
  const dependents = impact.data.dependents.map((d) => named(scope, d));
  if (dependents.length === 0) return flipInOrder(targets);
  const q = ask({ mode: 'dependents-on-disable', targets: [...targets], dependents });
  const { choice } = await q.answer;
  if (choice === 'cancel') return 'cancelled';
  // «Only this one» leaves the dependents enabled; «Disable all» takes them along.
  const flip = choice === 'secondary' ? targets : [...dependents.map(asTarget), ...targets];
  return finish(q.id, flipInOrder(flip));
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
    return finish(q.id, applyEnabled(scope, targets, true, { bulk, ordered: false }));
  }
  // In the backend's safe enable order — a mod comes on after any of them it needs — so nothing
  // comes on without what it needs; the targets too may need each other.
  const { order } = impact.data;
  const flipInOrder = (flip: readonly ModOpTarget[]) =>
    applyEnabled(scope, inOrder(order, flip), true, { bulk, ordered: true });
  const requirements = impact.data.requirements.map((r) => named(scope, r));
  if (requirements.length === 0) return flipInOrder(targets);
  const q = ask({ mode: 'enable-with-requirements', targets: [...targets], requirements });
  const { choice } = await q.answer;
  if (choice === 'cancel') return 'cancelled';
  // «Only this one» leaves the requirements disabled; «Enable together» takes them along.
  const flip = choice === 'secondary' ? targets : [...requirements.map(asTarget), ...targets];
  return finish(q.id, flipInOrder(flip));
}

// The optional "also remove unneeded libraries" question (bulk). A failed lookup offers nothing:
// the restrictive direction — nothing extra is removed, which is what "no" would have done — and
// the removal the user asked for goes ahead. `own`: the flow's dependents dialog, spinning while
// this runs, whose place the question takes.
async function withOrphans(
  scope: ModOpScope,
  targets: readonly ModOpTarget[],
  own: number | null,
): Promise<ModOpTarget[] | null> {
  const r = await settleCall(() => commands.modsFindOrphans(scope.instanceId, shas(targets)));
  const orphans = r.ok ? r.data : [];
  if (orphans.length === 0) return [...targets];
  const { choice, alsoRemove } = await ask({ mode: 'orphans', targets: [...targets], orphans }, own)
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
    // Dependents are disabled, never removed (D3), in the backend's safe disable order; «Only
    // this one» leaves them enabled.
    if (choice === 'primary') alsoDisable = inOrder(impact.data.order, dependents.map(asTarget));
  }
  try {
    const removing = opts.offerOrphans ? await withOrphans(scope, targets, dialog) : [...targets];
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
  // In the backend's safe disable order, each before any of them it needs. They may need each
  // other, but the removal already broke them, so a failure does not end the run: every one gets
  // its try, and as few as possible stay on without what they need.
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
    // In reverse of the order they went off — the safe disable order put each before what it
    // needs — so a provider comes back first; the first failure ends the run, so nothing comes
    // back on without what it needs.
    const back = await flipAll(scope.instanceId, [...reEnable].reverse(), true, true);
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
