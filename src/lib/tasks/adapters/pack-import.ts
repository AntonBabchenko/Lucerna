// Runner adapter: wraps `runImport` (the frontend-orchestrated modpack
// import call) and feeds its two progress channels into the task registry.
//
// `typedError` re-throws real `Error` instances instead of resolving to
// `{status:'error'}` (see the bottom of `$lib/ipc/bindings`), so a bridge
// failure can throw straight out of `runImport`. `finish()` runs in a
// `finally` — same discipline `op-queue.svelte.ts` uses in `processNext` —
// so a thrown error still lands the task in a terminal state instead of
// wedging it as permanently running.

import type { ModpackProgress, ProgressTick } from '$lib/ipc/bindings';
import type { ImportOutcome } from '$lib/ops/import-runner';
import { runImport } from '$lib/ops/import-runner';
import type { ModpackImportRequest } from '$lib/modpacks/import-request';
import { finish, start, upsertProgress } from '../registry.svelte';
import {
  advanceProgressDisplay,
  canShowRate,
  emptyProgressDisplay,
  toTaskRate,
} from '../rate';
import type { TaskProgress } from '../types';

/** Throttle window for the byte-rate EWMA — matches ServerHostingTab's
 *  DISPLAY_REFRESH_MS so every progress readout in the app updates at the
 *  same cadence. */
const RATE_REFRESH_MS = 1000;

/** Turn one `(phase, bytes)` tick into the registry's `phase`/`progress`
 *  shape. The per-mod byte tick (when present and honest — a total > 0) wins
 *  over the coarse phase's file count: it is the finer-grained signal and is
 *  what today's OperationsView already renders as the progress bar. Falls
 *  back to the coarse phase's file count for phases that carry one
 *  (`installing_file`, `extracting_overrides`), else no numeric progress. */
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

/** Import a modpack as a `pack-import` task. No instance exists yet when the
 *  task starts (it is created mid-run), so `scope` stays empty for the whole
 *  task lifetime — `Task.scope` is fixed at `start()` and never revised. */
export async function importModpack(
  name: string,
  request: ModpackImportRequest,
): Promise<ImportOutcome> {
  const id = `pack-import-${crypto.randomUUID()}`;
  start({
    id,
    kind: 'pack-import',
    scope: {},
    title: name,
    phase: null,
    progress: null,
    lane: 'serial',
  });

  let display = emptyProgressDisplay();

  try {
    const outcome = await runImport(request, (phase, bytes) => {
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

    if (outcome.status === 'ok') {
      finish(id, { state: 'ok', details: outcome.details });
    } else if (outcome.status === 'partial') {
      finish(id, { state: 'partial' });
    } else {
      finish(id, { state: 'failed' });
    }
    return outcome;
  } catch (e) {
    finish(id, { state: 'failed' });
    return { status: 'error', message: e instanceof Error ? e.message : String(e) };
  }
}
