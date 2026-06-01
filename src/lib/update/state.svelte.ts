// Cross-component state for the auto-update flow. Mirrors the
// rune-in-a-.svelte.ts idiom used by $lib/settings/state.svelte.
import { commands, type UpdateInfo } from '$lib/ipc/bindings';
import { pushActionToast, pushInfo, dismiss } from '$lib/toasts/toasts.svelte';
import { formatError } from '$lib/ipc/format-error';

export const updateState = $state<{ value: UpdateInfo | null }>({ value: null });

/** Start the install: re-check + download + verify + launch happen in the
 *  backend, which exits the app on success. On failure, surface a sticky
 *  warning toast with a "download manually" action. */
export async function runUpdate(): Promise<void> {
  const progress = pushInfo('Downloading update…');
  const r = await commands.updateInstall();
  dismiss(progress);
  if (r.status !== 'ok') {
    const url = updateState.value?.release_url;
    pushActionToast(
      'warning',
      "Couldn't verify the update",
      {
        label: 'Open release page',
        run: () => void import('@tauri-apps/plugin-opener').then((m) => url && m.openUrl(url)),
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
