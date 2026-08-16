// Cross-component state for the auto-update flow. Mirrors the
// rune-in-a-.svelte.ts idiom used by $lib/settings/state.svelte.
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { commands, events, type UpdateInfo } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import {
  dismiss,
  pushActionToast,
  pushProgress,
  updateToastProgress,
} from '$lib/toasts/toasts.svelte';
import { openExternalHttps } from '$lib/ui/safe-open';

export const updateState = $state<{ value: UpdateInfo | null }>({ value: null });

/** True while an install is in flight. Read by the UI to disable Update
 *  controls; guards `runUpdate` against re-entry (double-click → two
 *  download→verify→spawn chains). */
export const updateInstalling = $state<{ value: boolean }>({ value: false });

/** Start the update action.
 *
 *  On platforms with in-app install (Windows) `installer` is present:
 *  re-check + download + verify + launch happen in the backend, which exits
 *  the app on success; on failure we surface a sticky warning with a
 *  "download manually" action.
 *
 *  On notify-only platforms (Linux) `installer` is null — there is no in-app
 *  install, so we open the GitHub release page and let the user update via
 *  their package manager or a fresh AppImage. Re-entrant calls while an
 *  install is already running are ignored. */
export async function runUpdate(): Promise<void> {
  if (updateInstalling.value) return;

  const info = updateState.value;
  if (info && info.installer === null) {
    if (info.release_url) {
      void openExternalHttps(info.release_url);
    }
    return;
  }

  updateInstalling.value = true;
  const tr = get(t);
  const installerUrl = info?.installer?.url;
  const toastId = pushProgress(tr('page.update.downloading'));

  // Surface a failure identically whether the command returned an Err or
  // something threw (e.g. the event listener failed to register): reset the
  // in-flight flag and offer the release page. Never leave the Update button
  // soft-locked or the progress toast leaked.
  const showFailure = (detail: string) => {
    updateInstalling.value = false;
    const url = updateState.value?.release_url;
    pushActionToast(
      'warning',
      tr('page.update.verifyFailed'),
      {
        label: tr('settings.general.updates.openReleasePage'),
        run: () => {
          if (url) void openExternalHttps(url);
        },
      },
      [detail],
    );
  };

  let un: (() => void) | undefined;
  try {
    // The installer download already emits DownloadProgress events; filter to
    // the installer URL so mod/JRE downloads don't move this bar.
    un = await events.downloadProgress.listen(({ payload }) => {
      if (payload.url !== installerUrl) return;
      const total = payload.bytes_total;
      const done = payload.bytes_done ?? 0;
      updateToastProgress(toastId, total && total > 0 ? Math.min(1, done / total) : null);
    });
    const r = await commands.updateInstall();
    // On success the backend launched the installer and called app.exit(0), so
    // this may not run; on a returned Err, surface it.
    if (r.status !== 'ok') showFailure(formatError(r.error));
  } catch (e) {
    // A thrown error (e.g. listen() IPC rejecting) — clean up and report it,
    // rather than leaving the flag stuck and the toast leaked.
    showFailure(String(e));
  } finally {
    un?.();
    dismiss(toastId);
  }
  // On success the backend launched the installer and called app.exit(0);
  // nothing to do here.
}

/** User dismissed the toast: remember this version so we don't nag again. */
export async function dismissUpdate(version: string): Promise<void> {
  await commands.updateDismiss(version);
  updateState.value = null;
}
