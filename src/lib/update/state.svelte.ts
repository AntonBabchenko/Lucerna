// Cross-component state for the auto-update flow. Mirrors the
// rune-in-a-.svelte.ts idiom used by $lib/settings/state.svelte.
import { commands, type UpdateInfo } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { dismiss, pushActionToast, pushInfo } from '$lib/toasts/toasts.svelte';

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
      void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(info.release_url));
    }
    return;
  }

  updateInstalling.value = true;
  const progress = pushInfo('Downloading update…');
  const r = await commands.updateInstall();
  dismiss(progress);
  if (r.status !== 'ok') {
    // Allow another attempt after a failure (on success the app exits).
    updateInstalling.value = false;
    const url = updateState.value?.release_url;
    pushActionToast(
      'warning',
      "Couldn't verify the update",
      {
        label: 'Open release page',
        run: () => {
          if (url) void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(url));
        },
      },
      [formatError(r.error)],
    );
  }
  // On success the backend launched the installer and called app.exit(0);
  // nothing to do here.
}

/** User dismissed the toast: remember this version so we don't nag again. */
export async function dismissUpdate(version: string): Promise<void> {
  await commands.updateDismiss(version);
  updateState.value = null;
}
