// Call-wrapping adapters for mod install and mod update. Previously (see
// git history at `d8c1d7a9`) this was a single GLOBAL event listener on
// `modInstallProgress` / `modInstalled` / `modInstallFailed` — necessary
// because `mods_install_with_deps` has many call sites that don't thread a
// task id through, and because that shape structurally could not tell an
// install apart from an update: `mods_update_one`
// (src-tauri/src/commands/mods.rs) emits the exact same three events with
// the exact same payload shape (`instance_id` + `project_id`, no
// operation-kind discriminator), so the old adapter always tagged its task
// `kind: 'mod-install'` — even on the ticks that actually originate from a
// mod UPDATE.
//
// The frontend does not need the wire payload to know which one it is: it
// is the one calling `mods_install_with_deps` or `mods_update_one`, so it
// already knows. Fix (same shape `game-install.ts` uses for the analogous
// verify/repair-vs-install ambiguity): wrap the CALL for identity (`kind`)
// and terminal state (ok/failed), and keep the global event only for the
// mid-flight `current`/`total` counter — the field (added in `d0455056`)
// that makes an honest "N of M" progress readout possible at all.
// `total === 0` means "still resolving the install set" — manifest extras
// download before `install_seq` is assembled, see `install_batch.rs` — and
// renders as indeterminate (`progress: null`), never a misleading "0 of 0".
//
// Unlike `game-install.ts`'s `installProgress` (which carries a resolved
// `version_id`, not the `instanceId` the frontend already knows, so
// `game-install.ts` documents it has no cheap way to filter),
// `ModInstallProgress` DOES carry `instance_id` — so the per-call listener
// below filters to it. That closes the one cross-talk gap `game-install.ts`
// cannot: a second install/update racing on a DIFFERENT instance can no
// longer bleed progress numbers into this one. A second install/update
// racing the SAME instance concurrently can still cross-talk on the
// numbers (both would pass the filter) — no worse than `game-install.ts`'s
// documented limitation, and de-duplicating concurrent ops on one instance
// is a caller concern (à la `op-queue.svelte.ts`'s enqueue guards), not
// this adapter's.
//
// `modInstalled` / `modInstallFailed` are no longer read at all: the
// awaited command's `Result` is the terminal-state signal now, and it is
// strictly more reliable than those events ever were — an install this
// adapter did not itself initiate can no longer be mistaken for one of
// ours, because there is nothing left here that reacts to a bare event.
//
// DROP-IN REPLACEMENTS for `commands.modsInstallWithDeps` /
// `commands.modsUpdateOne` (same shape `game-install.ts` uses for
// `installGame`/`commands.installInstance`): both wrappers resolve (or
// reject) with EXACTLY the same `Result` shape/behavior as the command they
// replace, and a real thrown `Error` (a bridge failure — see `typedError`'s
// doc comment at the bottom of `$lib/ipc/bindings`, which rethrows those
// instead of resolving to `{status:'error'}`) still propagates. That is what
// makes every call site a one-line swap: its existing `res.status ===
// 'error'` / `formatError(res.error)` handling keeps compiling and behaving
// identically, because `res` really is the same type. Task registration is
// a pure side effect layered on top — `finish()` runs before either the
// return or the rethrow, so the task always reaches a terminal state.
// (There used to be bespoke `ModInstallOutcome` / `ModUpdateOutcome` types
// here with a pre-formatted `message: string` on the error branch; they
// existed only because nothing called these adapters yet. Collapsing them to
// the commands' own `Result` types is what actually makes them wireable into
// the 13 real call sites without restructuring any of them.)

import type { ModVersion_Deserialize, VersionRef } from '$lib/ipc/bindings';
import { commands, events } from '$lib/ipc/bindings';
import { finish, start, upsertProgress } from '../registry.svelte';
import type { TaskProgress } from '../types';

function progressFor(current: number, total: number): TaskProgress | null {
  if (total === 0) return null;
  return { current, total, unit: 'files' };
}

/** Attach the progress listener for exactly the span of one call, filtered
 *  to `instanceId` so a concurrent install/update on a DIFFERENT instance
 *  can never move this task's counter. Mirrors `installGame`'s
 *  subscribe-before-invoke shape. */
async function withModInstallProgress<T>(
  id: string,
  instanceId: string,
  run: () => Promise<T>,
): Promise<T> {
  // Best-effort: a progress subscription that cannot attach must never cost
  // the user the install itself. Progress is decoration; the call is the
  // point. Same discipline `op-queue.svelte.ts` has always applied to its own
  // listener — without it, any context with no Tauri event bridge (vitest, a
  // transient bridge failure) turns a working install into a failed one.
  let unlisten: (() => void) | null = null;
  try {
    unlisten = await events.modInstallProgress.listen((e) => {
      const p = e.payload;
      if (p.instance_id !== instanceId) return;
      upsertProgress(id, { phase: p.phase, progress: progressFor(p.current, p.total) });
    });
  } catch {
    // Task still runs, just without a live counter.
  }
  try {
    return await run();
  } finally {
    unlisten?.();
  }
}

/** Install a mod (plus any missing dependencies) as a `mod-install` task.
 *  Same signature shape and EXACT same return type as
 *  `commands.modsInstallWithDeps` (plus the `name` every call-wrapping
 *  adapter takes for the task's display title — see `installGame`), so a
 *  caller can swap `commands.modsInstallWithDeps(instanceId, primary,
 *  optionalDeps)` for `installModWithDeps(instanceId, name, primary,
 *  optionalDeps)` with no other change. */
export async function installModWithDeps(
  instanceId: string,
  name: string,
  primary: VersionRef,
  optionalDeps: VersionRef[],
): ReturnType<typeof commands.modsInstallWithDeps> {
  const id = `mod-install-${crypto.randomUUID()}`;
  start({
    id,
    kind: 'mod-install',
    scope: { instanceId },
    title: name,
    phase: null,
    progress: null,
    lane: 'concurrent',
  });

  try {
    const r = await withModInstallProgress(id, instanceId, () =>
      commands.modsInstallWithDeps(instanceId, primary, optionalDeps),
    );
    if (r.status === 'ok') {
      // `InstallSummary.details` is the same `TaskDetail[]` shape the
      // registry's report modal already renders for pack-import/pack-update
      // (see pack-import.ts's identical `finish(id, { state: 'ok', details:
      // ... })`) — one row per installed jar, so the finished task carries
      // the same per-mod breakdown the old UI toast showed.
      // `?.` deliberately: the report rows are decoration, and a summary
      // without them must still land the task as `ok` rather than throwing
      // out of the install. Reading `r.data.details` unguarded turned a
      // successful install into an unhandled rejection on #352's CI.
      finish(id, { state: 'ok', details: r.data?.details ?? null });
    } else {
      finish(id, { state: 'failed' });
    }
    return r;
  } catch (e) {
    // A real thrown Error (bridge failure) — land the task in a terminal
    // state, then rethrow unchanged so this call behaves exactly like
    // `commands.modsInstallWithDeps` would have.
    finish(id, { state: 'failed' });
    throw e;
  }
}

/** Update one already-installed mod to a new version as a `mod-update`
 *  task — the exact backend call (`mods_update_one`) the old event adapter
 *  could not tell apart from an install. Here it is unambiguous by
 *  construction: this function is only ever called to perform an update.
 *  Same signature shape and EXACT same return type as
 *  `commands.modsUpdateOne` (plus the `name` display title), so a caller can
 *  swap `commands.modsUpdateOne(instanceId, oldSha1, target)` for
 *  `updateMod(instanceId, name, oldSha1, target)` with no other change. */
export async function updateMod(
  instanceId: string,
  name: string,
  oldSha1: string,
  target: ModVersion_Deserialize,
): ReturnType<typeof commands.modsUpdateOne> {
  const id = `mod-update-${crypto.randomUUID()}`;
  start({
    id,
    kind: 'mod-update',
    scope: { instanceId },
    title: name,
    phase: null,
    progress: null,
    lane: 'concurrent',
  });

  try {
    const r = await withModInstallProgress(id, instanceId, () =>
      commands.modsUpdateOne(instanceId, oldSha1, target),
    );
    finish(id, { state: r.status === 'ok' ? 'ok' : 'failed' });
    return r;
  } catch (e) {
    // A real thrown Error (bridge failure) — land the task in a terminal
    // state, then rethrow unchanged so this call behaves exactly like
    // `commands.modsUpdateOne` would have.
    finish(id, { state: 'failed' });
    throw e;
  }
}
