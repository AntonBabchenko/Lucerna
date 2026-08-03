// Runner adapter for game install. Unlike the other four adapters, there is
// no dedicated runner module: the frontend calls `commands.installInstance`
// directly (see +page.svelte's install handler) and progress arrives on the
// GLOBAL `installProgress` Tauri event, not a per-call `Channel` argument.
//
// This MUST stay a runner adapter, not an event adapter: verify/repair
// invoke the exact same backend `install_version`, so their `installProgress`
// events are byte-for-byte indistinguishable from a game install's — a
// generic subscriber that mints a `game-install` task from every
// `installProgress` event would mint a phantom task on every repair. The
// event also carries a `version_id` (the resolved effective version — see
// `effective_version_id` in `src-tauri/src/instances/status.rs`, which
// synthesises a composite id for modded loaders), not the `instanceId` the
// frontend already knows, and the two differ for every non-vanilla instance
// — so this adapter has no cheap way to filter incoming events to "its own"
// call.
//
// Wrapping the call directly sidesteps that ambiguity: we know this is a
// game install because we are the one invoking it. The listener is attached
// only for the span of this call (subscribe right before invoking, unlisten
// in `finally`), so it never mints a task from a repair that happens to run
// while nothing here is in flight. A repair racing a concurrent game install
// can still cross-talk on the progress NUMBERS (both ride the same global
// event) — that is a known, pre-existing limitation shared with today's
// PhaseStatusRow.svelte, not a regression this adapter introduces.
//
// `typedError` re-throws real `Error` instances instead of resolving to
// `{status:'error'}` (see the bottom of `$lib/ipc/bindings`), so a bridge
// failure can throw straight out of `installInstance`. `finish()` runs in a
// `finally` so a thrown error still lands the task in a terminal state
// instead of wedging it as permanently running.

import { commands, events } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { finish, start, upsertProgress } from '../registry.svelte';

export type GameInstallOutcome = { status: 'ok' } | { status: 'error'; message: string };

/** Install (idempotently) an instance's version as a `game-install` task. */
export async function installGame(instanceId: string, name: string): Promise<GameInstallOutcome> {
  const id = `game-install-${crypto.randomUUID()}`;
  start({
    id,
    kind: 'game-install',
    scope: { instanceId },
    title: name,
    phase: null,
    progress: null,
    lane: 'concurrent',
  });

  let unlisten: (() => void) | null = null;
  try {
    unlisten = await events.installProgress.listen((event) => {
      const p = event.payload;
      // No `unit: 'bytes'` here even though the event carries `bytes_done`:
      // it only advances at whole-file completion by the file's declared
      // size — a staircase, not a transfer rate. See rate.ts's doc comment
      // on `canShowRate`, which excludes game install for the same reason.
      upsertProgress(id, {
        phase: p.phase,
        progress: { current: p.files_done, total: p.files_total, unit: 'files' },
      });
    });

    const r = await commands.installInstance(instanceId);
    if (r.status === 'ok') {
      finish(id, { state: 'ok' });
      return { status: 'ok' };
    }
    finish(id, { state: 'failed' });
    return { status: 'error', message: formatError(r.error) };
  } catch (e) {
    finish(id, { state: 'failed' });
    return { status: 'error', message: e instanceof Error ? e.message : String(e) };
  } finally {
    unlisten?.();
  }
}
