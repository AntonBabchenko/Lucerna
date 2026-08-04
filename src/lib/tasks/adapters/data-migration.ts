// Event adapter for data-root relocation. Subscribes to the GLOBAL
// `dataMigrationProgress` event rather than wrapping a call: the actual
// migration calls (`commands.setDataLocation` / `commands.adoptDataLocation`)
// are owned end-to-end by `$lib/settings/StoragePanel.svelte` — directory
// picker, plan classification, confirm dialog, THEN the call — which this
// task must not touch or rewire. This adapter runs alongside that flow, not
// in place of it.
//
// TERMINATE-BY-EXIT (the reason this task exists as its own note): on
// success, neither `setDataLocation` nor `adoptDataLocation` ever returns —
// the backend calls `app.restart()` and the whole process (frontend +
// backend) exits and relaunches (see StoragePanel.svelte's `confirmPending`:
// "On success neither command returns... reaching here means it failed
// before that point."). There is no "migration complete" event, and there
// never CAN be one on the success path — the process that would emit it is
// gone. So this adapter never calls `finish()`: a task it starts stays
// `running` for the rest of the session. On success that is exactly
// correct — the session ends anyway, so nothing is left showing a stale
// spinner to a live user, and the next launch starts with a fresh module
// singleton. On a FAILURE (the command resolves with an error instead of
// restarting), the session continues and this task IS left stuck `running`
// with no way for this adapter to close it — `StoragePanel.svelte` already
// surfaces that failure itself (`migrationError`), but nothing here can
// mirror it without wrapping the call, which is deliberately out of scope
// for this event-only adapter. Known, accepted gap for v1; closing it is a
// call-wrapping migration task's job, same shape as `server-upload.ts`'s
// failure-detection gap.
//
// No natural instance/server scope (a data-root relocation isn't per
// instance or per server) and at most one ever runs at a time, so `scope`
// stays `{}` for the task's whole life — same convention pack-import.ts uses
// when no instance exists yet.

import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { events } from '$lib/ipc/bindings';
import { start, upsertProgress } from '../registry.svelte';
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
