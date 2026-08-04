// Call-wrapping adapters for instance verify and repair. Previously (see
// git history at `d8c1d7a9`) this was a GLOBAL event listener on
// `verifyProgress` — deliberately shaped that way because
// `op-queue.svelte.ts` already owned the verify/repair CALL (it awaits
// `commands.verifyInstance` / `commands.repairInstance` directly to drive
// its own queue), and the old adapter was told not to touch that module.
// That event-only shape had two consequences its own comment documented
// honestly, both fixed here the same way `game-install.ts` fixes the
// analogous game-install-vs-repair ambiguity:
//
// (1) SAME-EVENT AMBIGUITY: `VerifyProgress` carries no field
// distinguishing a plain verify from a repair — `repair_instance_report`
// (src-tauri/src/verify/repair.rs) calls the SAME `verify_instance_report`
// (src-tauri/src/verify/scan.rs) a plain verify uses. The old adapter
// therefore always tagged its task `kind: 'verify'`, even for a repair.
// This adapter instead wraps `commands.verifyInstance` /
// `commands.repairInstance` directly, so `kind` is `'verify'` or `'repair'`
// by construction — we know which one it is because we are the one
// invoking it, not because the wire payload told us.
//
// (2) COMPLETION AMBIGUITY: there is no single "done" tick either. A
// repair-with-problems calls `verify_instance_report` TWICE (once before
// the fix, once after, to re-verify), so `VerifyPhase::Complete` fires
// twice, and repair's own true last tick (`emit_repairing(1, 1)`) arrives
// AFTER that second `Complete`. The old event-only adapter had to GUESS a
// terminal tick from this pattern (`phase === 'complete'` OR (`phase ===
// 'repairing'` and `files_done === files_total`)) — its own comment noted
// that guess could close the task a little early and reopen a short-lived
// follow-up. Because this adapter now AWAITS the real command, it needs no
// guess at all: the awaited `Result` is the one unambiguous terminal
// signal, so `finish()` is called exactly once, from exactly one place.
// The event stream is read ONLY for the file counter now (`upsertProgress`
// on every tick, unconditionally) — the double-`Complete` / late-
// `repairing(1,1)` pattern the previous agent found is preserved exactly as
// documented above (nothing here changes the backend), but it is now
// purely a mid-flight PROGRESS-DISPLAY curiosity (the phase/counter can
// visibly restart partway through a repair's re-verify pass) rather than a
// task-completion correctness concern.
//
// `typedError` re-throws real `Error` instances rather than resolving to
// `{status:'error'}` (see the bottom of `$lib/ipc/bindings`), so a bridge
// failure can throw straight out of the command call. `finish()` runs on
// every exit path (both `Result` branches, and the `catch`) so a thrown
// error still lands the task in a terminal state instead of wedging it as
// permanently running — same discipline the runner adapters in `89523af5`
// use.
//
// This module still does not touch `op-queue.svelte.ts` or its consumers:
// it is a second, independent caller of the same backend commands, exactly
// like `installGame` is a second, independent caller of
// `commands.installInstance` alongside +page.svelte's own call. Wiring one
// to replace the other is a later migration task's job.
//
// Keyed by nothing persistent (no module-level map): each call owns its own
// task id and its own listener span, so no cross-call bookkeeping is
// needed. Lane 'serial' still enforces the one-at-a-time rule structurally
// via the registry's `start()` (see `registry.svelte.ts`), matching
// today's `op-queue.svelte.ts` behaviour of one strictly-serial queue.

import type { VerifyReport } from '$lib/ipc/bindings';
import { commands, events } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { finish, start, TaskCancelledError, upsertProgress } from '../registry.svelte';

export type IntegrityOutcome =
  | { status: 'ok'; data: VerifyReport }
  | { status: 'error'; message: string }
  // Cancellation happens at the gate, before `call` is ever invoked (see the
  // `catch` in `runIntegrityCall`) — a queued task dropped via
  // `cancelQueued`, not a real failure.
  | { status: 'cancelled' };

/** Attach the progress listener for exactly the span of one call, filtered
 *  to `instanceId`. Every tick just updates the counter — no tick is ever
 *  treated as terminal here (see the module doc comment on why that guess
 *  is gone). */
async function withVerifyProgress<T>(
  id: string,
  instanceId: string,
  run: () => Promise<T>,
): Promise<T> {
  const unlisten = await events.verifyProgress.listen((e) => {
    const p = e.payload;
    if (p.instance_id !== instanceId) return;
    upsertProgress(id, {
      phase: p.phase,
      progress: { current: p.files_done, total: p.files_total, unit: 'files' },
    });
  });
  try {
    return await run();
  } finally {
    unlisten();
  }
}

async function runIntegrityCall(
  kind: 'verify' | 'repair',
  instanceId: string,
  name: string,
  call: () => ReturnType<typeof commands.verifyInstance>,
): Promise<IntegrityOutcome> {
  const id = `${kind}-${crypto.randomUUID()}`;

  try {
    // Gate: a second serial task queues here and this call does not
    // proceed to attach the progress listener or invoke `call` until the
    // registry promotes it — see `registry.svelte.ts`'s `start()` doc
    // comment.
    await start({
      id,
      kind,
      scope: { instanceId },
      title: name,
      phase: null,
      progress: null,
      lane: 'serial',
    });

    const r = await withVerifyProgress(id, instanceId, call);
    if (r.status === 'ok') {
      finish(id, { state: 'ok' });
      return { status: 'ok', data: r.data };
    }
    finish(id, { state: 'failed' });
    return { status: 'error', message: formatError(r.error) };
  } catch (e) {
    // A queued task dropped via `cancelQueued` before it ever ran — not a
    // failure, so it gets its own terminal state instead of falling into
    // the generic error branch (which used to surface as a failure toast).
    if (e instanceof TaskCancelledError) {
      finish(id, { state: 'cancelled' });
      return { status: 'cancelled' };
    }
    finish(id, { state: 'failed' });
    return { status: 'error', message: e instanceof Error ? e.message : String(e) };
  }
}

/** Verify an instance's integrity as a `verify` task. */
export function runVerify(instanceId: string, name: string): Promise<IntegrityOutcome> {
  return runIntegrityCall('verify', instanceId, name, () => commands.verifyInstance(instanceId));
}

/** Repair an instance as a `repair` task — always tagged `kind: 'repair'`,
 *  even on a healthy instance where the fix step is skipped entirely (a
 *  no-op repair is still the repair the user asked for, not a verify). */
export function runRepair(instanceId: string, name: string): Promise<IntegrityOutcome> {
  return runIntegrityCall('repair', instanceId, name, () => commands.repairInstance(instanceId));
}
