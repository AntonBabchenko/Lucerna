// Two entry points for SFTP server uploads, because ownership is split for
// now:
//
// `ensureServerUploadListener` is the ORIGINAL event adapter (see git
// history at `d8c1d7a9`), UNCHANGED: it subscribes to the GLOBAL
// `serverUploadProgress` event rather than wrapping a call, because the
// actual upload call (`commands.serverUpload`) is owned end-to-end by
// `$lib/servers/server-state.svelte.ts`'s `upload()` — which this task must
// not touch or rewire (five surfaces read its `uploads` Map today:
// ServerHostingTab, ServersPanel, ServerSidebarSection, ModeSwitcher, and
// its own tests; `isUploading()` disables the restart button while a
// transfer is in flight). For an upload started that way, this listener is
// still the ONLY signal available, and it still has the KNOWN GAP its
// original comment documented: `ServerUploadProgress` is the ONLY event the
// backend emits for an upload (see
// `src-tauri/src/servers_runtime/transfer.rs`) — there is no
// `server-upload-done` / `server-upload-failed` counterpart the way mod
// install has `modInstalled` / `modInstallFailed`. So a task minted from a
// bare tick can only ever reach `state: 'ok'` (inferred from `files_done
// === files_total`); a failed or cancelled upload driven purely by
// `server-state.svelte.ts` still cannot be detected here. Closing that
// needs the migration task to make `upload()` itself call this module's
// `uploadToServer` instead of `commands.serverUpload` directly.
//
// `uploadToServer` is the new, correct "wrap the call" shape (same as
// `installGame`): it invokes `commands.serverUpload` itself, so it knows
// the real terminal `Result` — a rejected/error Result reaches
// `finish(id, { state: 'failed' })` instead of leaving the task running
// forever, closing the gap above for whoever adopts it. It mirrors
// `server-state.svelte.ts`'s own `upload()` classification of
// `upload_cancelled` / `sftp_host_key_mismatch` as a silent `'cancelled'`
// outcome (never `'failed'`) — those two are the UI's re-trust-dialog /
// user-cancel affordance, and a `finish(id, { state: 'failed' })` here
// would make a future toast surface reading task state double-report what
// that dialog already owns. It attaches its OWN listener for the span of
// its call, filtered to `serverId` (mirrors `mod-install.ts`'s per-call,
// per-instance shape) rather than sharing `ensureServerUploadListener`'s
// module state, so the two entry points never race each other's `finish()`.
// Nothing calls `uploadToServer` yet — wiring `upload()` to it is a later
// migration task's job, same as `installGame` is unwired to +page.svelte's
// own install handler today.
//
// A completed task is left in its terminal `state` (never deleted) — the
// registry already keeps finished tasks in `taskList()` (see
// `registry.svelte.ts`'s `finish()`/`MAX_RETAINED_DETAILS`; only
// `taskFor()` hides them).
//
// Keyed by `server_id`. Lane 'concurrent': uploads to different servers run
// in parallel today (server-state.svelte.ts has no cross-server lock) and
// must keep doing so.

import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { commands, events } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
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

// Mirrors `server-state.svelte.ts`'s `upload()`: these two error kinds are
// the UI's own re-trust-dialog / cancel affordance, not a genuine failure.
const SILENT_ERROR_KINDS = new Set(['upload_cancelled', 'sftp_host_key_mismatch']);

export type ServerUploadOutcome =
  | { status: 'ok' }
  | { status: 'cancelled' }
  | { status: 'error'; message: string };

/** Upload a server's world/mods/config to its configured SFTP host as a
 *  `server-upload` task — the call-wrapping shape described in the module
 *  doc comment above. Not called by any production code yet; see that
 *  comment for why. */
export async function uploadToServer(
  serverId: string,
  name: string,
  acceptNewHostKey: boolean,
  skipWorlds: boolean,
  password: string | null,
  resume: boolean,
): Promise<ServerUploadOutcome> {
  const id = `server-upload-${crypto.randomUUID()}`;
  start({
    id,
    kind: 'server-upload',
    scope: { serverId },
    title: name,
    phase: null,
    progress: null,
    lane: 'concurrent',
  });

  let display = emptyProgressDisplay();
  try {
    const unlisten = await events.serverUploadProgress.listen((e) => {
      const p = e.payload;
      if (p.server_id !== serverId) return;
      const progress = progressFor(p.bytes_done, p.bytes_total, p.files_done, p.files_total);
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
      upsertProgress(id, { phase: p.current_file, progress, rate });
    });
    try {
      const r = await commands.serverUpload(
        serverId,
        acceptNewHostKey,
        skipWorlds,
        password,
        resume,
      );
      if (r.status === 'ok') {
        finish(id, { state: 'ok' });
        return { status: 'ok' };
      }
      if (SILENT_ERROR_KINDS.has(r.error.kind)) {
        finish(id, { state: 'cancelled' });
        return { status: 'cancelled' };
      }
      finish(id, { state: 'failed' });
      return { status: 'error', message: formatError(r.error) };
    } finally {
      unlisten();
    }
  } catch (e) {
    finish(id, { state: 'failed' });
    return { status: 'error', message: e instanceof Error ? e.message : String(e) };
  }
}
