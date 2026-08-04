// Runner adapter: wraps `runClone` (single-channel — phase only, no separate
// byte tick) and feeds its progress into the task registry.
//
// `typedError` re-throws real `Error` instances instead of resolving to
// `{status:'error'}` (see the bottom of `$lib/ipc/bindings`), so a bridge
// failure can throw straight out of `runClone`. `finish()` runs in a
// `finally` — same discipline `op-queue.svelte.ts` uses in `processNext` —
// so a thrown error still lands the task in a terminal state instead of
// wedging it as permanently running.

import type { CloneOutcome } from '$lib/instances/clone-runner';
import { runClone } from '$lib/instances/clone-runner';
import type { CloneRequest } from '$lib/instances/clone-request';
import { finish, start, upsertProgress } from '../registry.svelte';

/** Clone an instance as a `clone` task. Scoped to the SOURCE instance (not
 *  the not-yet-created clone) — `request.sourceId` is the same key
 *  `op-queue.svelte.ts` dedupes on today, and keeping the task scoped to it
 *  lets a source-instance-scoped UI (à la IntegritySection's `taskFor`
 *  lookup) reflect "a clone of this instance is already running". */
export async function cloneInstance(name: string, request: CloneRequest): Promise<CloneOutcome> {
  const id = `clone-${crypto.randomUUID()}`;

  try {
    // Gate: a second serial task queues here and this call does not
    // proceed to `runClone` until the registry promotes it — see
    // `registry.svelte.ts`'s `start()` doc comment.
    await start({
      id,
      kind: 'clone',
      scope: { instanceId: request.sourceId },
      title: name,
      phase: null,
      progress: null,
      lane: 'serial',
    });

    const outcome = await runClone(request, (phase) => {
      if (phase === null) {
        upsertProgress(id, { phase: null, progress: null });
        return;
      }
      upsertProgress(id, {
        phase: phase.category,
        progress: { current: phase.current, total: phase.total, unit: 'files' },
      });
    });

    finish(id, { state: outcome.status === 'ok' ? 'ok' : 'failed' });
    return outcome;
  } catch (e) {
    finish(id, { state: 'failed' });
    return { status: 'error', message: e instanceof Error ? e.message : String(e) };
  }
}
