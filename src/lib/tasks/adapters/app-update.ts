// Two entry points for the launcher self-update, because ownership is split
// for now:
//
// `ensureAppUpdateListener` is the ORIGINAL event adapter (see git history
// at `d8c1d7a9`), UNCHANGED: it subscribes to the GLOBAL `downloadProgress`
// event rather than wrapping a call, because the actual update call
// (`commands.updateInstall`) is owned end-to-end by
// `$lib/update/state.svelte.ts`'s `runUpdate()` — nag toast, "download
// manually" fallback on failure, dismissal — which this task must not touch
// or rewire. It reads the EXPORTED `updateState` (not calling anything on
// it) purely to know which URL is "the installer", for the same filter
// `runUpdate()` already applies: `downloadProgress` is truly global (mods,
// JRE, assets, and the installer all share it), so a tick only belongs to
// this adapter when its `url` matches `updateState.value?.installer?.url` —
// copied verbatim from `runUpdate()`'s own `if (payload.url !==
// installerUrl) return;` filter. For an update started that way, this
// listener is still the only signal available, and it still has the
// property its original comment documented: it never calls `finish()`,
// because on SUCCESS the backend spawns the installer and calls
// `app.exit(0)` — `runUpdate()`'s own comment: "this may not run" past the
// awaited call. A task minted from a bare tick therefore stays `running`
// for the rest of the session on success (exactly correct) but ALSO stays
// stuck `running` on a FAILURE driven purely by `runUpdate()` (which already
// surfaces that failure itself via a toast — this shadow task just can't
// mirror it). Closing that needs the migration task to make `runUpdate()`
// call this module's `runAppUpdate` instead of the raw command.
//
// KNOWN, NOT WORSENED UPSTREAM LIMITATION (applies to both entry points
// below): the updater downloads TWO artifacts (the installer plus, per
// `UpdateInfo`'s own fields, presumably a checksums/signature file) but
// only the installer's URL is ever filtered for — `sha256sums`/
// `cosign_bundle` downloads move the SAME global `downloadProgress` stream
// without either entry point here (or `runUpdate`) ever showing them. This
// reproduces that exact filter rather than "fixing" it — a real fix means
// the backend tagging which artifact a tick belongs to, not a frontend
// guess.
//
// `runAppUpdate` is the new, correct "wrap the call" shape (same as
// `installGame`): it invokes `commands.updateInstall` itself, so it knows
// the real terminal `Result` — a rejected/error Result reaches
// `finish(id, { state: 'failed' })` instead of leaving the task running
// forever. On an 'ok' Result it deliberately does NOT call
// `finish(id, { state: 'ok' })`: per the terminate-by-exit fact above, a
// real 'ok' resolution cannot happen in production (the process would
// already be exiting), so calling `finish` there would be inventing a
// completion this call was never designed to report. If an 'ok' Result is
// ever actually observed (only conceivable under test), the task is left
// `running` — the same outcome the passive listener already produces on
// success. It attaches its OWN listener for the span of its call, filtered
// to the installer URL exactly like `ensureAppUpdateListener` does, rather
// than sharing that listener's module state, so the two entry points never
// race each other's `finish()`. Nothing calls `runAppUpdate` yet — wiring
// `runUpdate()` to it is a later migration task's job, same as `installGame`
// is unwired to +page.svelte's own install handler today.
//
// No natural instance/server scope and at most one update ever runs at a
// time, so `scope` stays `{}` for the task's whole life.

import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { commands, events } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { updateState } from '$lib/update/state.svelte';
import { finish, start, upsertProgress } from '../registry.svelte';
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

export type AppUpdateOutcome = { status: 'ok' } | { status: 'error'; message: string };

/** Install the launcher self-update as an `app-update` task — the
 *  call-wrapping shape described in the module doc comment above. Not
 *  called by any production code yet; see that comment for why. */
export async function runAppUpdate(): Promise<AppUpdateOutcome> {
  const id = `app-update-${crypto.randomUUID()}`;
  start({
    id,
    kind: 'app-update',
    scope: {},
    title: get(t)('tasks.kind.appUpdate'),
    phase: null,
    progress: null,
    lane: 'modal',
  });

  try {
    const installerUrl = updateState.value?.installer?.url;
    const unlisten = await events.downloadProgress.listen((e) => {
      if (!installerUrl || e.payload.url !== installerUrl) return;
      upsertProgress(id, { progress: progressFor(e.payload.bytes_done, e.payload.bytes_total) });
    });
    try {
      const r = await commands.updateInstall();
      if (r.status === 'error') {
        finish(id, { state: 'failed' });
        return { status: 'error', message: formatError(r.error) };
      }
      // Unreachable in production — see the module doc comment: success
      // means the backend already launched the installer and called
      // `app.exit(0)`, so this resolution never arrives. Leave the task
      // 'running' rather than inventing a completion this call was never
      // designed to report.
      return { status: 'ok' };
    } finally {
      unlisten();
    }
  } catch (e) {
    finish(id, { state: 'failed' });
    return { status: 'error', message: e instanceof Error ? e.message : String(e) };
  }
}
