// Event adapter for SFTP server uploads. Subscribes to the GLOBAL
// `serverUploadProgress` event rather than wrapping a call: the actual
// upload call (`commands.serverUpload`) is owned end-to-end by
// `$lib/servers/server-state.svelte.ts`'s `upload()` — which this task must
// not touch or rewire (five surfaces read its `uploads` Map today:
// ServerHostingTab, ServersPanel, ServerSidebarSection, ModeSwitcher, and
// its own tests; `isUploading()` disables the restart button while a
// transfer is in flight). This adapter runs ALONGSIDE that store, not in
// place of it — wiring the two together is a later migration task's job.
//
// KNOWN GAP — no failure/cancellation signal: `ServerUploadProgress` is the
// ONLY event the backend emits for an upload (see
// `src-tauri/src/servers_runtime/transfer.rs`) — there is no
// `server-upload-done` / `server-upload-failed` counterpart the way mod
// install has `modInstalled` / `modInstallFailed`. `server-state.svelte.ts`
// only learns success/error/cancellation from the AWAITED
// `commands.serverUpload` call's `Result`, which this event-only adapter has
// no access to. So this adapter can only ever finish a task with `state:
// 'ok'` (inferred from `files_done === files_total`, the one honest
// completion signal the event stream offers) — it never produces 'failed' or
// 'cancelled'. In particular it CANNOT replicate
// `server-state.svelte.ts`'s deliberate demotion of `upload_cancelled` /
// `sftp_host_key_mismatch` to a silent `cancelled` phase (so the host-key
// re-trust dialog keeps sole ownership of that message) — there is simply no
// error payload here to demote. This adapter never pushes a toast either, so
// it cannot double-report anything; it just cannot report a failure at all.
// Closing this gap needs the migration task to make `upload()` itself call
// `start`/`upsertProgress`/`finish` (like the five runner adapters do),
// which is exactly the kind of "wrap the call" shape this file deliberately
// avoids for now.
//
// A completed task is left in its terminal `state` (never deleted) — the
// registry already keeps finished tasks in `taskList()` (see
// `registry.svelte.ts`'s `finish()`/`MAX_RETAINED_DETAILS`; only
// `taskFor()` hides them), so the "ok" outcome persists exactly the way
// `server-state.svelte.ts`'s own `done` phase does today. It's only the
// 'failed'/'cancelled' persistence this adapter cannot produce, per the gap
// above.
//
// Keyed by `server_id`. Lane 'concurrent': uploads to different servers run
// in parallel today (server-state.svelte.ts has no cross-server lock) and
// must keep doing so.

import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { events } from '$lib/ipc/bindings';
import { finish, start, upsertProgress } from '../registry.svelte';
import {
  advanceProgressDisplay,
  canShowRate,
  emptyProgressDisplay,
  toTaskRate,
  type ProgressDisplay,
} from '../rate';
import type { TaskProgress } from '../types';

/** Throttle window for the byte-rate EWMA — matches ServerHostingTab's own
 *  DISPLAY_REFRESH_MS so every progress readout in the app updates at the
 *  same cadence. */
const RATE_REFRESH_MS = 1000;

let listenerInit = false;
// serverId -> task id for the upload task currently tracked for it.
const activeTaskId = new Map<string, string>();
// serverId -> throttled byte-rate display, so concurrent uploads to
// different servers don't share (and corrupt) one EWMA state.
const rateDisplay = new Map<string, ProgressDisplay>();

function progressFor(
  bytesDone: number | null,
  bytesTotal: number | null,
  filesDone: number,
  filesTotal: number,
): TaskProgress | null {
  const total = bytesTotal ?? 0;
  if (total > 0) return { current: bytesDone ?? 0, total, unit: 'bytes' };
  if (filesTotal > 0) return { current: filesDone, total: filesTotal, unit: 'files' };
  return null;
}

/** Lazily attach the global listener. Idempotent and safe to call from
 *  anywhere repeatedly. Wrapped in try/catch exactly like
 *  `op-queue.svelte.ts`'s `ensureListener` — a module-load subscription
 *  throws under vitest, where there is no Tauri runtime, so callers must
 *  invoke this explicitly rather than relying on an import-time side
 *  effect. `listenUntilDestroyed` (`$lib/ipc/listen.ts`) does not fit here:
 *  it ties teardown to a component's `onDestroy`, but this is a module-level
 *  singleton with no owning component, same as `server-state.svelte.ts`'s
 *  own `init()`. */
export function ensureServerUploadListener(): void {
  if (listenerInit) return;
  listenerInit = true;
  try {
    events.serverUploadProgress
      .listen((e) => {
        const p = e.payload;
        const progress = progressFor(p.bytes_done, p.bytes_total, p.files_done, p.files_total);

        let rate = null;
        if (progress !== null && canShowRate(progress)) {
          const prev = rateDisplay.get(p.server_id) ?? emptyProgressDisplay();
          const display = advanceProgressDisplay(
            prev,
            progress.current,
            progress.total,
            Date.now(),
            RATE_REFRESH_MS,
          );
          rateDisplay.set(p.server_id, display);
          rate = toTaskRate(display);
        }

        let id = activeTaskId.get(p.server_id);
        if (id === undefined) {
          id = `server-upload-${crypto.randomUUID()}`;
          activeTaskId.set(p.server_id, id);
          start({
            id,
            kind: 'server-upload',
            scope: { serverId: p.server_id },
            title: get(t)('tasks.kind.serverUpload'),
            phase: p.current_file,
            progress,
            lane: 'concurrent',
          });
          if (rate !== null) upsertProgress(id, { rate });
        } else {
          upsertProgress(id, { phase: p.current_file, progress, rate });
        }

        if (p.files_total > 0 && p.files_done === p.files_total) {
          activeTaskId.delete(p.server_id);
          rateDisplay.delete(p.server_id);
          finish(id, { state: 'ok' });
        }
      })
      .catch(() => {});
  } catch {
    /* no Tauri runtime (vitest) — listener inert */
  }
}

/** Test-only reset of the module singleton. */
export function __resetServerUploadAdapterForTest(): void {
  listenerInit = false;
  activeTaskId.clear();
  rateDisplay.clear();
}
