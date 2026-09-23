// Cross-component state for the auto-update flow. Mirrors the
// rune-in-a-.svelte.ts idiom used by $lib/settings/state.svelte.
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { commands, events, type Error as IpcError, type UpdateInfo } from '$lib/ipc/bindings';
import { describeStoreError, formatError } from '$lib/ipc/format-error';
import { isActiveTask, taskList } from '$lib/tasks/registry.svelte';
import {
  dismiss,
  pushActionToast,
  pushInfo,
  pushProgress,
  pushWarning,
  updateToast,
  updateToastProgress,
} from '$lib/toasts/toasts.svelte';
import { openExternalHttps } from '$lib/ui/safe-open';

export const updateState = $state<{ value: UpdateInfo | null }>({ value: null });

/** True while an install is in flight. Read by the UI to disable Update
 *  controls; guards `runUpdate` against re-entry (double-click → two
 *  download→verify→spawn chains). */
export const updateInstalling = $state<{ value: boolean }>({ value: false });

/** Where the in-flight install is, from the backend's `UpdateInstallPhase` event; null before the
 *  first phase and outside an install. The button and the progress toast follow it, so neither
 *  says "Installing…" during what is a download. */
export type UpdatePhase = 'downloading' | 'verifying' | 'launching';
export const updatePhase = $state<{ value: UpdatePhase | null }>({ value: null });

type Block = 'running' | 'busy' | 'unknown';

const BLOCK_KEYS = {
  running: 'errors.updateBlocked.running',
  busy: 'errors.updateBlocked.busy',
  unknown: 'errors.updateBlocked.unknown',
} as const;

const PHASE_KEYS = {
  downloading: 'page.update.phase.downloading',
  verifying: 'page.update.phase.verifying',
  launching: 'page.update.phase.launching',
} as const;

/** Lucerna closes to install an update, and the exit hook force-kills every running game and
 *  server. So before asking the backend, refuse while this window has a task in flight (the
 *  backend cannot see an instance being created) and while the backend's observer says anything
 *  but an exact 'none' — a rejection or a malformed answer is "could not tell", which is a no. */
async function blockedBy(): Promise<Block | null> {
  if (taskList().some(isActiveTask)) return 'busy';
  try {
    const answer: unknown = await commands.restartBlocked();
    if (answer === 'none') return null;
    if (answer === 'running' || answer === 'busy') return answer;
  } catch {
    // Not wrapped in typedError: a failure is a rejection.
  }
  return 'unknown';
}

/** A refusal is not a failure: the sentence says what to do, and there is no "download it
 *  yourself" action — the user closes what runs and presses the button again. */
function refuse(block: Block): void {
  updateInstalling.value = false;
  updatePhase.value = null;
  pushWarning(get(t)(BLOCK_KEYS[block]));
}

function isBlockedError(e: IpcError): e is Extract<IpcError, { kind: 'update_blocked' }> {
  return e.kind === 'update_blocked';
}

/** Start the update action.
 *
 *  On platforms with in-app install (Windows, Linux AppImage) `installer` is
 *  present: re-check + download + verify + launch happen in the backend, which
 *  exits the app on success; on failure we surface a sticky warning with a
 *  "download manually" action.
 *
 *  On notify-only platforms `installer` is null — there is no in-app install,
 *  so we open the GitHub release page and let the user update via their
 *  package manager or a fresh AppImage. Re-entrant calls while an install is
 *  already running are ignored. */
export async function runUpdate(): Promise<void> {
  if (updateInstalling.value) return;

  const info = updateState.value;
  // Before the gate: opening a web page closes nothing, and a notify-only user with a game
  // running must still reach it.
  if (info && info.installer === null) {
    if (info.release_url) {
      void openExternalHttps(info.release_url);
    }
    return;
  }

  updateInstalling.value = true;
  updatePhase.value = null;
  const blocked = await blockedBy();
  if (blocked !== null) {
    refuse(blocked);
    return;
  }

  const tr = get(t);
  const installerUrl = info?.installer?.url;
  const toastId = pushProgress(tr('page.update.downloading'));

  // Surface a failure identically whether the command returned an Err or
  // something threw (e.g. the event listener failed to register): reset the
  // in-flight flag and offer the release page. Never leave the Update button
  // soft-locked or the progress toast leaked.
  const showFailure = (headline: string, detail: string) => {
    updateInstalling.value = false;
    updatePhase.value = null;
    const url = updateState.value?.release_url;
    pushActionToast(
      'warning',
      headline,
      {
        label: tr('settings.general.updates.openReleasePage'),
        run: () => {
          if (url) void openExternalHttps(url);
        },
      },
      [detail],
    );
  };
  // Only a failed verification is headlined as one: a network blip during the download is not a
  // bad signature, and must not read like something is wrong with the release.
  const headlineFor = (e: IpcError | null) =>
    tr(
      e?.kind === 'update_verification_failed'
        ? 'page.update.verifyFailed'
        : 'page.update.installFailed',
    );

  let unProgress: (() => void) | undefined;
  let unPhase: (() => void) | undefined;
  try {
    // The installer download already emits DownloadProgress events; filter to
    // the installer URL so mod/JRE downloads don't move this bar. Both listeners
    // register before the command: if either cannot, the run aborts — one rule.
    unProgress = await events.downloadProgress.listen(({ payload }) => {
      if (payload.url !== installerUrl) return;
      const total = payload.bytes_total;
      const done = payload.bytes_done ?? 0;
      updateToastProgress(toastId, total && total > 0 ? Math.min(1, done / total) : null);
    });
    unPhase = await events.updateInstallPhase.listen(({ payload }) => {
      updatePhase.value = payload.phase;
      updateToast(toastId, { title: tr(PHASE_KEYS[payload.phase]) });
      // The bar means bytes; past the download it would be a lie.
      if (payload.phase !== 'downloading') updateToastProgress(toastId, null);
    });
    const r = await commands.updateInstall();
    if (r.status === 'ok') {
      // On success the backend launched the installer and called app.exit(0), so this does not
      // run. Reaching it means the re-check found nothing newer (a release yanked after the
      // offer): free the button — it used to stay "Installing…" for the rest of the session.
      updateInstalling.value = false;
      updatePhase.value = null;
      updateState.value = null;
      pushInfo(tr('page.update.alreadyCurrent'));
    } else if (isBlockedError(r.error)) {
      // Something started during the download and the backend refused before spawning the
      // installer: the same warning as the pre-check, not a failure.
      refuse(r.error.block === 'running' || r.error.block === 'busy' ? r.error.block : 'unknown');
    } else {
      showFailure(headlineFor(r.error), formatError(r.error));
    }
  } catch (e) {
    // A thrown error (e.g. listen() IPC rejecting) — clean up and report it,
    // rather than leaving the flag stuck and the toast leaked.
    showFailure(headlineFor(null), describeStoreError(e));
  } finally {
    unProgress?.();
    unPhase?.();
    dismiss(toastId);
  }
}

/** How long the startup "new version available" toast stays before it
 *  auto-hides — paused while it is hovered or focused. Hiding is not skipping:
 *  it comes back next launch. */
export const UPDATE_TOAST_TTL_MS = 5000;

export type SkipOutcome = { ok: true; skipped: string | null } | { ok: false; error: string };

/** "Skip this version": the startup check stops offering `version`. Returns
 *  the skip as the backend persisted it. */
export async function skipUpdate(version: string): Promise<SkipOutcome> {
  // The offer stays: a skipped version is still available — only the startup
  // notice stops. Settings → Updates keeps showing it, with Stop skipping.
  try {
    const r = await commands.updateDismiss(version);
    return r.status === 'ok'
      ? { ok: true, skipped: r.data }
      : { ok: false, error: formatError(r.error) };
  } catch (e) {
    return { ok: false, error: describeStoreError(e) };
  }
}

/** "Stop skipping": the startup check offers the skipped version again. */
export async function stopSkipping(): Promise<SkipOutcome> {
  try {
    const r = await commands.updateClearDismissed();
    return r.status === 'ok'
      ? { ok: true, skipped: r.data }
      : { ok: false, error: formatError(r.error) };
  } catch (e) {
    return { ok: false, error: describeStoreError(e) };
  }
}

/** The startup notice for an available update: Update now, and a readable
 *  Skip this version. The × and the auto-hide only close it. */
export function showUpdateToast(info: UpdateInfo): number {
  const tr = get(t);
  const version = info.latest;
  return pushActionToast(
    'info',
    tr('page.update.available', { version }),
    { label: tr('page.update.actionLabel'), run: () => void runUpdate() },
    [tr('page.update.currentVersion', { version: info.current })],
    {
      secondary: {
        label: tr('settings.general.updates.skip'),
        run: () => {
          void skipUpdate(version).then((r) => {
            // The toast is already gone; the failure must still be said.
            if (!r.ok) pushWarning(get(t)('page.update.dismissFailed'), [r.error]);
          });
        },
      },
      ttlMs: UPDATE_TOAST_TTL_MS,
    },
  );
}
