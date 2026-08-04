// Event adapter for the launcher self-update. Subscribes to the GLOBAL
// `downloadProgress` event rather than wrapping a call: the actual update
// call (`commands.updateInstall`) is owned end-to-end by
// `$lib/update/state.svelte.ts`'s `runUpdate()` — nag toast, "download
// manually" fallback on failure, dismissal — which this task must not touch
// or rewire. This adapter runs alongside that flow, reading its EXPORTED
// `updateState` (not calling anything on it) purely to know which URL is
// "the installer" for this same filter `runUpdate()` already applies:
// `downloadProgress` is truly global (mods, JRE, assets, and the installer
// all share it), so a tick only belongs to this adapter when its `url`
// matches `updateState.value?.installer?.url` — copied verbatim from
// `runUpdate()`'s own `if (payload.url !== installerUrl) return;` filter.
//
// KNOWN, NOT WORSENED UPSTREAM LIMITATION: the updater downloads TWO
// artifacts (the installer plus, per `runUpdate`'s own fields, presumably a
// checksums/signature file) but only the installer's URL is ever filtered
// for — `sha256sums`/`cosign_bundle` downloads move the SAME global
// `downloadProgress` stream without this adapter (or `runUpdate`) ever
// showing them. This adapter reproduces that exact filter rather than
// "fixing" it, per instruction — a real fix means the backend tagging which
// artifact a tick belongs to, not a frontend guess.
//
// TERMINATE-BY-EXIT: on success the backend spawns the installer and calls
// `app.exit(0)` — `runUpdate()`'s own comment: "this may not run" past the
// awaited call. There is no "update complete" event, and there never CAN be
// one on the success path — the process that would emit it is gone. So this
// adapter never calls `finish()`: a task it starts stays `running` for the
// rest of the session, which is correct on success (the session ends
// anyway). On a FAILURE (the command resolves with an error instead of
// exiting), `runUpdate()` already surfaces that itself (a warning toast with
// a "download manually" action) — but this shadow task has no way to mirror
// that outcome without wrapping the call, which is deliberately out of scope
// here. Known, accepted gap for v1, same shape as `data-migration.ts`'s and
// `server-upload.ts`'s failure-detection gaps.
//
// No natural instance/server scope and at most one update ever runs at a
// time, so `scope` stays `{}` for the task's whole life.

import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { events } from '$lib/ipc/bindings';
import { updateState } from '$lib/update/state.svelte';
import { start, upsertProgress } from '../registry.svelte';
import type { TaskProgress } from '../types';

let listenerInit = false;
let activeTaskId: string | null = null;

function progressFor(bytesDone: number | null, bytesTotal: number | null): TaskProgress | null {
  const total = bytesTotal ?? 0;
  if (total <= 0) return null;
  return { current: bytesDone ?? 0, total, unit: 'bytes' };
}

/** Lazily attach the global listener. Idempotent and safe to call from
 *  anywhere repeatedly. Wrapped in try/catch exactly like
 *  `op-queue.svelte.ts`'s `ensureListener` — a module-load subscription
 *  throws under vitest, where there is no Tauri runtime, so callers must
 *  invoke this explicitly rather than relying on an import-time side
 *  effect. `listenUntilDestroyed` (`$lib/ipc/listen.ts`) does not fit here:
 *  it ties teardown to a component's `onDestroy`, but this is a module-level
 *  singleton with no owning component. */
export function ensureAppUpdateListener(): void {
  if (listenerInit) return;
  listenerInit = true;
  try {
    events.downloadProgress
      .listen((e) => {
        const installerUrl = updateState.value?.installer?.url;
        if (!installerUrl || e.payload.url !== installerUrl) return;

        const progress = progressFor(e.payload.bytes_done, e.payload.bytes_total);
        if (activeTaskId === null) {
          activeTaskId = `app-update-${crypto.randomUUID()}`;
          start({
            id: activeTaskId,
            kind: 'app-update',
            scope: {},
            title: get(t)('tasks.kind.appUpdate'),
            phase: null,
            progress,
            lane: 'modal',
          });
        } else {
          upsertProgress(activeTaskId, { progress });
        }
      })
      .catch(() => {});
  } catch {
    /* no Tauri runtime (vitest) — listener inert */
  }
}

/** Test-only reset of the module singleton. */
export function __resetAppUpdateAdapterForTest(): void {
  listenerInit = false;
  activeTaskId = null;
}
