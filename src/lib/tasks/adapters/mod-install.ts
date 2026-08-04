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
// `typedError` re-throws real `Error` instances rather than resolving to
// `{status:'error'}` (see the bottom of `$lib/ipc/bindings`), so a bridge
// failure can throw straight out of the command call. Both exported
// functions wrap their call in try/catch so a thrown error still lands the
// task in a terminal `failed` state instead of wedging it as permanently
// running — same discipline the runner adapters in `89523af5` use.

import type { InstallSummary, ModVersion_Deserialize, VersionRef } from '$lib/ipc/bindings';
import { commands, events } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { finish, start, upsertProgress } from '../registry.svelte';
import type { TaskProgress } from '../types';

export type ModInstallOutcome =
  | { status: 'ok'; data: InstallSummary }
  | { status: 'error'; message: string };

export type ModUpdateOutcome = { status: 'ok' } | { status: 'error'; message: string };

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
  const unlisten = await events.modInstallProgress.listen((e) => {
    const p = e.payload;
    if (p.instance_id !== instanceId) return;
    upsertProgress(id, { phase: p.phase, progress: progressFor(p.current, p.total) });
  });
  try {
    return await run();
  } finally {
    unlisten();
  }
}

/** Install a mod (plus any missing dependencies) as a `mod-install` task. */
export async function installModWithDeps(
  instanceId: string,
  name: string,
  primary: VersionRef,
  optionalDeps: VersionRef[],
): Promise<ModInstallOutcome> {
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
      finish(id, { state: 'ok', details: r.data.details });
      return { status: 'ok', data: r.data };
    }
    finish(id, { state: 'failed' });
    return { status: 'error', message: formatError(r.error) };
  } catch (e) {
    finish(id, { state: 'failed' });
    return { status: 'error', message: e instanceof Error ? e.message : String(e) };
  }
}

/** Update one already-installed mod to a new version as a `mod-update`
 *  task — the exact backend call (`mods_update_one`) the old event adapter
 *  could not tell apart from an install. Here it is unambiguous by
 *  construction: this function is only ever called to perform an update. */
export async function updateMod(
  instanceId: string,
  name: string,
  oldSha1: string,
  target: ModVersion_Deserialize,
): Promise<ModUpdateOutcome> {
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
    if (r.status === 'ok') {
      finish(id, { state: 'ok' });
      return { status: 'ok' };
    }
    finish(id, { state: 'failed' });
    return { status: 'error', message: formatError(r.error) };
  } catch (e) {
    finish(id, { state: 'failed' });
    return { status: 'error', message: e instanceof Error ? e.message : String(e) };
  }
}
