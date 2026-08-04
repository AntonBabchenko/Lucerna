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
// DROP-IN REPLACEMENT for `commands.installInstance`: `installGame` resolves
// (or rejects) with EXACTLY the same `Result` shape/behavior as
// `commands.installInstance` itself — a bare `{status:'ok', data: null} |
// {status:'error', error: Error}`, and a real thrown `Error` (a bridge
// failure — see `typedError`'s doc comment at the bottom of
// `$lib/ipc/bindings`, which rethrows those instead of resolving to
// `{status:'error'}`) still propagates. That is what makes +page.svelte's
// `onInstall()` a one-line swap: its existing `result.status === 'error'` /
// `formatError(result.error)` handling keeps compiling and behaving
// identically, because `result` really is the same type. Task registration
// is a pure side effect layered on top — `finish()` runs before either the
// return or the rethrow, so the task always reaches a terminal state.
// (There used to be a bespoke `GameInstallOutcome` type here with a
// pre-formatted `message: string` on the error branch; it existed only
// because nothing called this adapter yet. Collapsing it to the command's
// own `Result` is what actually makes this wireable into +page.svelte
// without restructuring `onInstall()`.)

import { commands, events } from '$lib/ipc/bindings';
import { finish, start, upsertProgress } from '../registry.svelte';

/** Install (idempotently) an instance's version as a `game-install` task.
 *  Same signature shape and EXACT same return type as
 *  `commands.installInstance` (plus the `name` every other call-wrapping
 *  adapter in this module takes for the task's display title — see
 *  `runVerify`/`runRepair`/`importModpack`), so a caller can swap
 *  `commands.installInstance(id)` for `installGame(id, name)` with no other
 *  change. */
export async function installGame(
  instanceId: string,
  name: string,
): ReturnType<typeof commands.installInstance> {
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
    finish(id, { state: r.status === 'ok' ? 'ok' : 'failed' });
    return r;
  } catch (e) {
    // A real thrown Error (bridge failure) — land the task in a terminal
    // state, then rethrow unchanged so this call behaves exactly like
    // `commands.installInstance` would have.
    finish(id, { state: 'failed' });
    throw e;
  } finally {
    unlisten?.();
  }
}
