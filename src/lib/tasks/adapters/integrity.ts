// Event adapter for instance integrity checks (verify + repair). Subscribes
// to the GLOBAL `verifyProgress` event rather than wrapping a call:
// `op-queue.svelte.ts` already owns the verify/repair CALL (it awaits
// `commands.verifyInstance` / `commands.repairInstance` directly to learn
// success/failure and drive its own `running`/queue state), and this task
// must not touch that module or its consumers — this adapter only shadows
// the same event stream so the task registry carries a record too.
//
// SAME-EVENT AMBIGUITY (same shape as game-install.ts's documented
// `installProgress` cross-talk, and mod-install.ts's documented
// `mods_update_one` cross-talk): `VerifyProgress` carries no field
// distinguishing a plain verify from a repair.
// `repair_instance_report` (src-tauri/src/verify/repair.rs) calls the SAME
// `verify_instance_report` (src-tauri/src/verify/scan.rs) a plain verify
// uses, and only adds `VerifyPhase::Repairing` ticks for the fix step —
// which is SKIPPED ENTIRELY when `verify_instance_report` finds the instance
// already healthy (`repair_instance_report` returns early in that case, see
// repair.rs). So a "repair" that fixes nothing is byte-for-byte identical on
// the wire to a plain verify. This adapter always tags its task
// `kind: 'verify'`: honest for a plain verify and for a healthy-instance
// repair, and merely under-labels an actual repair's fix step (still shown,
// just under the 'verify' kind/label rather than 'repair'). Splitting them
// needs a backend change (an explicit operation kind or id threaded onto the
// event), not a frontend guess.
//
// COMPLETION AMBIGUITY: there is no single "done" tick either.
// `verify_instance_report` always ends with `VerifyPhase::Complete` — but a
// repair-with-problems calls it TWICE (once before the fix, once after, to
// re-verify — see `repair_instance_report`), and repair's own true last tick
// is `emit_repairing(1, 1)`, which arrives AFTER that second `Complete`. A
// tick is therefore treated as terminal when EITHER `phase === 'complete'`
// OR (`phase === 'repairing'` and `files_done === files_total`): the first
// case closes a plain verify (or a repair that needed no fix) at its one and
// only Complete; the second closes a repair's fix step. A targeted repair's
// per-download ticks can ALSO satisfy `files_done === files_total` before
// the operation's real last tick (e.g. the final item of a 2-item download
// batch) — that just closes the task a little early and lets the next
// tick(s) open (and, for the true final `repairing(1,1)` marker, immediately
// re-close) a short-lived follow-up task. Never wedges anything 'running'
// forever, which is the property that actually matters here.
//
// Keyed by `instance_id`. Lane 'serial': verify and repair queue behind each
// other today (`op-queue.svelte.ts`'s single `running` gate) — the registry
// enforces the same one-at-a-time rule structurally via the 'serial' lane
// (see `registry.svelte.ts`'s `start()`), so no extra bookkeeping is needed
// here for that part.

import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { events } from '$lib/ipc/bindings';
import { finish, start, upsertProgress } from '../registry.svelte';

let listenerInit = false;
// instanceId -> task id for the verify/repair task currently tracked for it.
const activeTaskId = new Map<string, string>();

function isTerminalTick(phase: string, filesDone: number, filesTotal: number): boolean {
  if (phase === 'complete') return true;
  return phase === 'repairing' && filesTotal > 0 && filesDone === filesTotal;
}

/** Lazily attach the global listener. Idempotent and safe to call from
 *  anywhere repeatedly. Wrapped in try/catch exactly like
 *  `op-queue.svelte.ts`'s `ensureListener` — a module-load subscription
 *  throws under vitest, where there is no Tauri runtime, so callers must
 *  invoke this explicitly rather than relying on an import-time side
 *  effect. `listenUntilDestroyed` (`$lib/ipc/listen.ts`) does not fit here:
 *  it ties teardown to a component's `onDestroy`, but this is a module-level
 *  singleton with no owning component, same as `op-queue.svelte.ts`. */
export function ensureIntegrityListener(): void {
  if (listenerInit) return;
  listenerInit = true;
  try {
    events.verifyProgress
      .listen((e) => {
        const p = e.payload;
        const progress = { current: p.files_done, total: p.files_total, unit: 'files' as const };
        let id = activeTaskId.get(p.instance_id);
        if (id === undefined) {
          id = `verify-${crypto.randomUUID()}`;
          activeTaskId.set(p.instance_id, id);
          start({
            id,
            kind: 'verify',
            scope: { instanceId: p.instance_id },
            title: get(t)('tasks.kind.verify'),
            phase: p.phase,
            progress,
            lane: 'serial',
          });
        } else {
          upsertProgress(id, { phase: p.phase, progress });
        }
        if (isTerminalTick(p.phase, p.files_done, p.files_total)) {
          activeTaskId.delete(p.instance_id);
          finish(id, { state: 'ok' });
        }
      })
      .catch(() => {});
  } catch {
    /* no Tauri runtime (vitest) — listener inert */
  }
}

/** Test-only reset of the module singleton. */
export function __resetIntegrityAdapterForTest(): void {
  listenerInit = false;
  activeTaskId.clear();
}
