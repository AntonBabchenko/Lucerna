// Event adapter for mod install. Unlike the five runner adapters (which wrap
// one specific call), this subscribes to the GLOBAL `modInstallProgress` /
// `modInstalled` / `modInstallFailed` events: `mods_install_with_deps` is
// invoked from several call sites (ModBrowseView, dependency dialogs, the
// modpack-update flow's optional-dep step, …) and none of them thread a task
// id through today, so there is no single call this module could wrap the
// way `installGame` wraps `commands.installInstance`. Mirrors how
// `PhaseStatusRow.svelte` already listens to this same event globally.
//
// SHARED-EVENT LIMITATION (same shape as game-install.ts's documented
// `installProgress` cross-talk with verify/repair): `mods_update_one`
// (src-tauri/src/commands/mods.rs) emits the exact same three events with
// the exact same payload shape (`instance_id` + `project_id`, no
// operation-kind discriminator) when updating a single mod. This adapter
// therefore always tags its task `kind: 'mod-install'`, even on the ticks
// that actually originate from a mod UPDATE — there is nothing in the wire
// payload to tell the two apart. Splitting them needs a backend change (an
// explicit `is_update` flag or an operation id), not a frontend heuristic.
// Not a regression introduced here: `PhaseStatusRow.svelte` has the
// identical blind spot today (it just shows the phase, unlabelled either
// way).
//
// `current`/`total` (added in `d0455056`) is what makes an honest "N of M"
// progress readout possible at all. `total === 0` means "still resolving the
// install set" — manifest extras download before `install_seq` is
// assembled, see `install_batch.rs` — and renders as indeterminate
// (`progress: null`), never a misleading "0 of 0".
//
// Keyed by `instance_id`: at most one tracked mod-install task per instance.
// Different instances install concurrently (`lane: 'concurrent'`), matching
// today's behaviour (nothing in the backend serializes mod installs across
// instances).

import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { events } from '$lib/ipc/bindings';
import { finish, start, upsertProgress } from '../registry.svelte';
import type { TaskProgress } from '../types';

let listenerInit = false;
// instanceId -> task id for the mod-install task currently tracked for it.
const activeTaskId = new Map<string, string>();

function progressFor(current: number, total: number): TaskProgress | null {
  if (total === 0) return null;
  return { current, total, unit: 'files' };
}

/** Lazily attach the three global listeners this adapter needs. Idempotent
 *  and safe to call from anywhere repeatedly. Wrapped in try/catch exactly
 *  like `op-queue.svelte.ts`'s `ensureListener` — a module-load subscription
 *  throws under vitest, where there is no Tauri runtime, so callers must
 *  invoke this explicitly rather than relying on an import-time side
 *  effect. `listenUntilDestroyed` (`$lib/ipc/listen.ts`) does not fit here:
 *  it ties teardown to a component's `onDestroy`, but this is a module-level
 *  singleton with no owning component, same as `op-queue.svelte.ts`. */
export function ensureModInstallListener(): void {
  if (listenerInit) return;
  listenerInit = true;
  try {
    events.modInstallProgress
      .listen((e) => {
        const p = e.payload;
        const progress = progressFor(p.current, p.total);
        const existing = activeTaskId.get(p.instance_id);
        if (existing === undefined) {
          const id = `mod-install-${crypto.randomUUID()}`;
          activeTaskId.set(p.instance_id, id);
          start({
            id,
            kind: 'mod-install',
            scope: { instanceId: p.instance_id },
            title: get(t)('tasks.kind.modInstall'),
            phase: p.phase,
            progress,
            lane: 'concurrent',
          });
        } else {
          upsertProgress(existing, { phase: p.phase, progress });
        }
      })
      .catch(() => {});

    // Both landing (`modInstalled`, once per jar) and failing
    // (`modInstallFailed`, once for the whole atomic batch) close out the
    // task for that instance. `modInstalled` fires once per installed jar in
    // a tight loop after the batch commits (see `install_batch.rs`), so
    // `finish()` can run more than once here for the same id — the registry
    // tolerates that (it just re-applies the same terminal patch), and by
    // the second call the id is already gone from `activeTaskId`, so a
    // stray late event for the SAME instance cannot resurrect an old task.
    events.modInstalled
      .listen((e) => {
        const id = activeTaskId.get(e.payload.instance_id);
        if (id === undefined) return;
        activeTaskId.delete(e.payload.instance_id);
        finish(id, { state: 'ok' });
      })
      .catch(() => {});

    events.modInstallFailed
      .listen((e) => {
        const id = activeTaskId.get(e.payload.instance_id);
        if (id === undefined) return;
        activeTaskId.delete(e.payload.instance_id);
        finish(id, { state: 'failed' });
      })
      .catch(() => {});
  } catch {
    /* no Tauri runtime (vitest) — listener inert */
  }
}

/** Test-only reset of the module singleton. */
export function __resetModInstallAdapterForTest(): void {
  listenerInit = false;
  activeTaskId.clear();
}
