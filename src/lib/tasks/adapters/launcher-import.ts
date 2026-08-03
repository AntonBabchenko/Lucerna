// Runner adapter: wraps `runLauncherImport` (single-channel — phase only, no
// separate byte tick) and feeds its progress into the task registry.
//
// `typedError` re-throws real `Error` instances instead of resolving to
// `{status:'error'}` (see the bottom of `$lib/ipc/bindings`), so a bridge
// failure can throw straight out of `runLauncherImport`. `finish()` runs in a
// `finally` — same discipline `op-queue.svelte.ts` uses in `processNext` —
// so a thrown error still lands the task in a terminal state instead of
// wedging it as permanently running.

import type {
  LauncherImportOutcome,
  LauncherImportRequest,
} from '$lib/instances/import/launcher-import-runner';
import { runLauncherImport } from '$lib/instances/import/launcher-import-runner';
import { finish, start, upsertProgress } from '../registry.svelte';

/** Import a foreign launcher instance as a `launcher-import` task. No
 *  Lucerna instance exists yet when the task starts (it is created
 *  mid-run), so `scope` stays empty for the whole task lifetime —
 *  `Task.scope` is fixed at `start()` and never revised. */
export async function importLauncherInstance(
  name: string,
  request: LauncherImportRequest,
): Promise<LauncherImportOutcome> {
  const id = `launcher-import-${crypto.randomUUID()}`;
  start({
    id,
    kind: 'launcher-import',
    scope: {},
    title: name,
    phase: null,
    progress: null,
    lane: 'serial',
  });

  try {
    const outcome = await runLauncherImport(request, (phase) => {
      if (phase === null) {
        upsertProgress(id, { phase: null, progress: null });
        return;
      }
      const progress =
        phase.phase === 'copying'
          ? { current: phase.current, total: phase.total, unit: 'files' as const }
          : null;
      upsertProgress(id, { phase: phase.phase, progress });
    });

    finish(id, { state: outcome.status === 'ok' ? 'ok' : 'failed' });
    return outcome;
  } catch (e) {
    finish(id, { state: 'failed' });
    return { status: 'error', message: e instanceof Error ? e.message : String(e) };
  }
}
