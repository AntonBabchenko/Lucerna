// Two entry points for data-root relocation, because ownership is split for
// now:
//
// `ensureDataMigrationListener` is the ORIGINAL event adapter (see git
// history at `d8c1d7a9`), UNCHANGED: it subscribes to the GLOBAL
// `dataMigrationProgress` event rather than wrapping a call, because the
// actual migration calls (`commands.setDataLocation` /
// `commands.adoptDataLocation`) are owned end-to-end by
// `$lib/settings/StoragePanel.svelte` — directory picker, plan
// classification, confirm dialog, THEN the call — which this task must not
// touch or rewire. For a migration started that way, this listener is
// still the only signal available, and it still has the property its
// original comment documented: it never calls `finish()`, because on
// SUCCESS neither command ever returns (the backend calls `app.restart()`
// and the whole process exits and relaunches — see StoragePanel.svelte's
// `confirmPending`: "On success neither command returns... reaching here
// means it failed before that point"). A task minted from a bare tick
// therefore stays `running` for the rest of the session on success (exactly
// correct — the session ends anyway) but ALSO stays stuck `running` on a
// FAILURE driven purely by StoragePanel.svelte, since this listener has no
// access to the command's `Result`. Closing that needs the migration task
// to make `confirmPending()` call this module's wrappers below instead of
// the raw commands.
//
// `resetDataLocation` / `migrateDataLocation` / `adoptDataLocationTask` are
// the new, correct "wrap the call" shape (same as `installGame`): each
// invokes its command itself, so it knows the real terminal `Result` — a
// rejected/error Result reaches `finish(id, { state: 'failed' })` instead
// of leaving the task running forever. On an 'ok' Result they deliberately
// do NOT call `finish(id, { state: 'ok' })`: per the terminate-by-exit fact
// above, a real 'ok' resolution cannot happen in production (the process
// would already be restarting), so calling `finish` there would be
// inventing a completion this call was never designed to report. If an
// 'ok' Result is ever actually observed (only conceivable under test), the
// task is left `running` — the same outcome the passive listener already
// produces on success. Nothing calls these wrappers yet — wiring
// `confirmPending()` to them is a later migration task's job, same as
// `installGame` is unwired to +page.svelte's own install handler today.
//
// No natural instance/server scope (a data-root relocation isn't per
// instance or per server) and at most one ever runs at a time, so `scope`
// stays `{}` for the task's whole life — same convention pack-import.ts uses
// when no instance exists yet.

import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { commands, events } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { finish, start, upsertProgress } from '../registry.svelte';
import type { TaskProgress } from '../types';

let listenerInit = false;
let activeTaskId: string | null = null;

function progressFor(copiedBytes: number | null, totalBytes: number | null): TaskProgress | null {
  const total = totalBytes ?? 0;
  if (total <= 0) return null;
  return { current: copiedBytes ?? 0, total, unit: 'bytes' };
}

/** Lazily attach the global listener. Idempotent and safe to call from
 *  anywhere repeatedly. Wrapped in try/catch exactly like
 *  `op-queue.svelte.ts`'s `ensureListener` — a module-load subscription
 *  throws under vitest, where there is no Tauri runtime, so callers must
 *  invoke this explicitly rather than relying on an import-time side
 *  effect. `listenUntilDestroyed` (`$lib/ipc/listen.ts`) does not fit here:
 *  it ties teardown to a component's `onDestroy`, but this is a module-level
 *  singleton with no owning component. */
export function ensureDataMigrationListener(): void {
  if (listenerInit) return;
  listenerInit = true;
  try {
    events.dataMigrationProgress
      .listen((e) => {
        const p = e.payload;
        const progress = progressFor(p.copied_bytes, p.total_bytes);
        if (activeTaskId === null) {
          activeTaskId = `data-migration-${crypto.randomUUID()}`;
          start({
            id: activeTaskId,
            kind: 'data-migration',
            scope: {},
            title: get(t)('tasks.kind.dataMigration'),
            phase: p.phase,
            progress,
            lane: 'modal',
          });
        } else {
          upsertProgress(activeTaskId, { phase: p.phase, progress });
        }
      })
      .catch(() => {});
  } catch {
    /* no Tauri runtime (vitest) — listener inert */
  }
}

/** Test-only reset of the module singleton. */
export function __resetDataMigrationAdapterForTest(): void {
  listenerInit = false;
  activeTaskId = null;
}

export type DataMigrationOutcome = { status: 'ok' } | { status: 'error'; message: string };

/** Shared body for the three call-wrapping entry points below. Attaches its
 *  own listener for the span of the call (mirrors `mod-install.ts`'s
 *  per-call shape) rather than sharing `ensureDataMigrationListener`'s
 *  module state, so the two entry points never race each other's
 *  `finish()`. */
async function runDataMigrationCall(
  call: () => ReturnType<typeof commands.setDataLocation>,
): Promise<DataMigrationOutcome> {
  const id = `data-migration-${crypto.randomUUID()}`;
  start({
    id,
    kind: 'data-migration',
    scope: {},
    title: get(t)('tasks.kind.dataMigration'),
    phase: null,
    progress: null,
    lane: 'modal',
  });

  try {
    const unlisten = await events.dataMigrationProgress.listen((e) => {
      const p = e.payload;
      upsertProgress(id, {
        phase: p.phase,
        progress: progressFor(p.copied_bytes, p.total_bytes),
      });
    });
    try {
      const r = await call();
      if (r.status === 'error') {
        finish(id, { state: 'failed' });
        return { status: 'error', message: formatError(r.error) };
      }
      // Unreachable in production — see the module doc comment: success
      // means `app.restart()` already fired and this resolution never
      // arrives. Leave the task 'running' rather than inventing a
      // completion this call was never designed to report.
      return { status: 'ok' };
    } finally {
      unlisten();
    }
  } catch (e) {
    finish(id, { state: 'failed' });
    return { status: 'error', message: e instanceof Error ? e.message : String(e) };
  }
}

/** Reset the data root to the default location as a `data-migration`
 *  task. Not called by any production code yet; see the module doc comment. */
export function resetDataLocation(): Promise<DataMigrationOutcome> {
  return runDataMigrationCall(() => commands.setDataLocation(null));
}

/** Relocate the data root to `path` (a real copy) as a `data-migration`
 *  task. Not called by any production code yet; see the module doc comment. */
export function migrateDataLocation(path: string): Promise<DataMigrationOutcome> {
  return runDataMigrationCall(() => commands.setDataLocation(path));
}

/** Adopt an existing data root at `path` (redirect write, no copy — see
 *  StoragePanel.svelte's `confirmPending`) as a `data-migration` task. Not
 *  called by any production code yet; see the module doc comment. */
export function adoptDataLocationTask(path: string): Promise<DataMigrationOutcome> {
  return runDataMigrationCall(() => commands.adoptDataLocation(path));
}
