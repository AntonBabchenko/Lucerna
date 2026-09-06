// Runner adapter: wraps `commands.worldMigrate` (one progress `Channel`,
// one `MigrationProgress` tick per step) and feeds it into the task
// registry. The shape is `./clone.ts`'s; the byte-rate handling for the
// copy phase is `./pack-import.ts`'s.
//
// `typedError` re-throws real `Error` instances instead of resolving to
// `{status:'error'}` (see the bottom of `$lib/ipc/bindings`), so a bridge
// failure can throw straight out of the command call. `finish()` runs on
// every exit path (both `Result` branches and the `catch`), so a thrown
// error still lands the task in a terminal state instead of wedging it as
// permanently running.
//
// Cancellation: a running migration cannot be cancelled — the backend has
// no cancellation token and the registry cancels only QUEUED tasks (spec
// §4.2). Nothing here has to enforce that: `registry.svelte.ts`'s `capsFor`
// derives `caps.cancellable` from state/lane and it is `false` for every
// running task. Cancelling this task while it is still queued rejects the
// `start()` gate below, so the command is never invoked.

import { Channel } from '@tauri-apps/api/core';
import type { MigrationMode, MigrationOutcome, MigrationProgress } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { advanceProgressDisplay, canShowRate, emptyProgressDisplay, toTaskRate } from '../rate';
import { finish, start, TaskCancelledError, upsertProgress } from '../registry.svelte';
import type { TaskProgress } from '../types';

/** `error` is the raw value — the IPC `Error` union from a `{status:'error'}`
 *  result, or whatever the bridge threw — so the caller can route an IPC
 *  error through `formatError` and anything else through its message.
 *  `cancelled` means the task was dropped via `cancelQueued` while still
 *  queued: the command never ran and nothing on disk changed. */
export type MigrateTaskOutcome =
  | { status: 'ok'; outcome: MigrationOutcome }
  | { status: 'error'; error: unknown }
  | { status: 'cancelled' };

/** Throttle window for the byte-rate EWMA — the same cadence
 *  `pack-import.ts` and ServerHostingTab's DISPLAY_REFRESH_MS use, so every
 *  progress readout in the app updates together. */
const RATE_REFRESH_MS = 1000;

type RegistryTick = { phase: string; progress: TaskProgress | null };

/** This tick's counter as a `TaskProgress`, or `null` when there is no
 *  honest one to draw. `MigrationProgress.current`/`.total` are
 *  `number | null` — specta renders every Rust `f64` that way — while
 *  `TaskProgress` takes plain numbers, so an unreadable counter must mean NO
 *  progress bar rather than a bar drawn from an invented zero. */
function counter(p: MigrationProgress, unit: TaskProgress['unit']): TaskProgress | null {
  if (p.current === null || p.total === null) return null;
  return { current: p.current, total: p.total, unit };
}

/** One wire tick → the registry's `phase`/`progress` shape. `copying` is
 *  the only phase whose counter is bytes (the tree copy on the copy path);
 *  `linking` and `backups` count files; `moving` (the O(1) rename) and
 *  `finalising` carry no honest counter, so no bar. Exhaustive over
 *  `MigrationPhase`: a phase added on the Rust side without a decision here
 *  fails to compile (the `never` guard in `default`). */
function translateProgress(p: MigrationProgress): RegistryTick {
  switch (p.phase) {
    case 'copying':
      return { phase: p.phase, progress: counter(p, 'bytes') };
    case 'linking':
    case 'backups':
      return { phase: p.phase, progress: counter(p, 'files') };
    case 'moving':
    case 'finalising':
      return { phase: p.phase, progress: null };
    default: {
      const unreachable: never = p.phase;
      return { phase: unreachable, progress: null };
    }
  }
}

/** True when the outcome carries something the user must read: a datapack left
 *  as a plain copy or copied rather than linked, skipped links, backups left
 *  behind, or a source that a move did not remove. The task strip then says
 *  `partial` — the operation finished, but not everything it set out to do —
 *  instead of an `ok` that the completion toast immediately contradicts. */
export function isPartialOutcome(o: MigrationOutcome): boolean {
  return (
    o.datapacks.some(
      (d) => d.result.kind === 'left_as_copy' || d.result.kind === 'copied_not_linked',
    ) ||
    o.links_skipped > 0 ||
    o.backups_left > 0 ||
    (o.source_state.kind !== 'removed' && o.source_state.kind !== 'untouched')
  );
}

/** Migrate one world as a `world-migrate` task. Scoped to the SOURCE
 *  instance: that is the instance whose Worlds tab the user is looking at,
 *  so `taskFor({ instanceId: fromInstance })` lets the dialog disable its
 *  confirm button (with a reason) while a migration of this source is
 *  queued or running. The backend's maintenance claim on BOTH instances is
 *  the real protection (spec §4.0); this scope is only the UI's mirror. */
export async function migrateWorld(
  title: string,
  req: { fromInstance: string; worldFolder: string; toInstance: string; mode: MigrationMode },
): Promise<MigrateTaskOutcome> {
  const id = `world-migrate-${crypto.randomUUID()}`;
  let display = emptyProgressDisplay();

  try {
    // Gate: a second serial task queues here and this call does not
    // proceed to the command until the registry promotes it — see
    // `registry.svelte.ts`'s `start()` doc comment.
    await start({
      id,
      kind: 'world-migrate',
      scope: { instanceId: req.fromInstance },
      title,
      phase: null,
      progress: null,
      lane: 'serial',
    });

    const ch = new Channel<MigrationProgress>();
    ch.onmessage = (p) => {
      const { phase, progress } = translateProgress(p);
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
      // `rate` stays null on every non-byte phase so a copy-phase speed
      // never lingers under "Reconnecting datapacks".
      upsertProgress(id, { phase, progress, rate });
    };

    const r = await commands.worldMigrate(
      req.fromInstance,
      req.worldFolder,
      req.toInstance,
      req.mode,
      ch,
    );
    if (r.status === 'ok') {
      finish(id, { state: isPartialOutcome(r.data) ? 'partial' : 'ok' });
      return { status: 'ok', outcome: r.data };
    }
    finish(id, { state: 'failed' });
    return { status: 'error', error: r.error };
  } catch (e) {
    // A queued task dropped via `cancelQueued` before it ever ran — not a
    // failure, so it gets its own terminal state instead of falling into
    // the generic error branch.
    if (e instanceof TaskCancelledError) {
      finish(id, { state: 'cancelled' });
      return { status: 'cancelled' };
    }
    finish(id, { state: 'failed' });
    return { status: 'error', error: e };
  }
}
