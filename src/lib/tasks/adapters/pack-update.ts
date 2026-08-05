// Runner adapter: wraps `runUpdate` (the "apply" step of a modpack update —
// see modpack-update-flow.svelte's `confirm()`) and feeds its two progress
// channels into the task registry. Direct mirror of pack-import.ts, same as
// update-runner.ts mirrors import-runner.ts.
//
// Only the `applying` phase is task-shaped: `prepare()`/the confirm dialog
// are interactive pre-steps that gate user consent, not background work — do
// not start a task for them. This module does not touch
// `modpack-update-flow.svelte.ts`; wiring `confirm()` to call this instead of
// `runUpdate` directly is a later task's job, and `confirm()` keeps returning
// the promise its three callers await.
//
// `typedError` re-throws real `Error` instances instead of resolving to
// `{status:'error'}` (see the bottom of `$lib/ipc/bindings`), so a bridge
// failure can throw straight out of `runUpdate`. `finish()` runs in a
// `finally` — same discipline `op-queue.svelte.ts` uses in `processNext` —
// so a thrown error still lands the task in a terminal state instead of
// wedging it as permanently running.

import type { ModpackProgress, ProgressTick } from '$lib/ipc/bindings';
import type { UpdateOutcome, UpdateProgressCb } from '$lib/modpacks/update-runner';
import { runUpdate } from '$lib/modpacks/update-runner';
import { advanceProgressDisplay, canShowRate, emptyProgressDisplay, toTaskRate } from '../rate';
import { finish, start, TaskCancelledError, upsertProgress } from '../registry.svelte';
import type { TaskProgress } from '../types';

/** Widens the runner's own outcome with the one status `runUpdate` can
 *  never produce itself: cancellation happens at the gate, before it is
 *  ever invoked (see the `catch` below) — so it belongs on the adapter's
 *  return type, not on `UpdateOutcome` (which `modpack-update-flow.svelte.ts`
 *  also consumes directly, with no gate in front of it at all). */
export type PackUpdateTaskOutcome = UpdateOutcome | { status: 'cancelled' };

/** Throttle window for the byte-rate EWMA — matches ServerHostingTab's
 *  DISPLAY_REFRESH_MS so every progress readout in the app updates at the
 *  same cadence. */
const RATE_REFRESH_MS = 1000;

/** Turn one `(phase, bytes)` tick into the registry's `phase`/`progress`
 *  shape. Identical rule to pack-import.ts's translator: the per-mod byte
 *  tick wins when honest (total > 0), else fall back to the coarse phase's
 *  file count for phases that carry one. */
function translateProgress(
  phase: ModpackProgress | null,
  bytes: ProgressTick | null,
): { phase: string | null; progress: TaskProgress | null } {
  const taskPhase = phase?.phase ?? null;
  if (bytes !== null && bytes.current !== null && bytes.total !== null && bytes.total > 0) {
    return {
      phase: taskPhase,
      progress: { current: bytes.current, total: bytes.total, unit: 'bytes' },
    };
  }
  if (
    phase !== null &&
    (phase.phase === 'installing_file' || phase.phase === 'extracting_overrides')
  ) {
    return {
      phase: taskPhase,
      progress: { current: phase.current, total: phase.total, unit: 'files' },
    };
  }
  return { phase: taskPhase, progress: null };
}

/** Apply a fetched-and-confirmed modpack update as a `pack-update` task. The
 *  instance already exists (an update targets one), so `scope.instanceId` is
 *  set for the whole task lifetime. */
export async function applyModpackUpdate(
  name: string,
  instanceId: string,
  tempPath: string,
  newVersionId: string,
  /** Forwarded verbatim so a caller that already renders its own in-screen
   *  progress keeps it. `createModpackUpdateFlow` drives the inline bar on
   *  three surfaces off this; registering a task must not take that away
   *  from them (spec D4: in-screen indicators keep their numbers). */
  onProgress?: UpdateProgressCb,
): Promise<PackUpdateTaskOutcome> {
  const id = `pack-update-${crypto.randomUUID()}`;
  let display = emptyProgressDisplay();

  try {
    // Gate: a second serial task queues here and this call does not
    // proceed to `runUpdate` until the registry promotes it — see
    // `registry.svelte.ts`'s `start()` doc comment.
    await start({
      id,
      kind: 'pack-update',
      scope: { instanceId },
      title: name,
      phase: null,
      progress: null,
      lane: 'serial',
    });

    const outcome = await runUpdate(instanceId, tempPath, newVersionId, (phase, bytes) => {
      onProgress?.(phase, bytes);
      const { phase: taskPhase, progress } = translateProgress(phase, bytes);
      let rate = null;
      if (progress !== null && canShowRate(progress)) {
        display = advanceProgressDisplay(
          display,
          progress.current,
          progress.total,
          Date.now(),
          RATE_REFRESH_MS,
        );
        rate = toTaskRate(display);
      }
      upsertProgress(id, { phase: taskPhase, progress, rate });
    });

    // Same `details` hand-off as pack-import.ts / mod-install.ts — an update
    // moves as many files as an import, so its report is worth as much.
    if (outcome.status === 'ok') {
      finish(id, { state: 'ok', details: outcome.details });
    } else {
      finish(id, { state: 'failed' });
    }
    return outcome;
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
