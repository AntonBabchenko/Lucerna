// Restart the launcher, or say why it did not happen. Shared by the data-move final dialog
// (DataMoveHost) and the recovery-session banner: in both places Restart is the way forward, so a
// failure has to end in the way out ("close Lucerna and open it again") and never in silence.

import type { Translate } from '$lib/i18n';
import { commands } from '$lib/ipc/bindings';
import { describeStoreError, formatError } from '$lib/ipc/format-error';

/** `null` = nothing to show (on success the process is replaced and this never returns at all).
 *  Otherwise the localized message, including the way out. */
export async function restartLauncherOrExplain(t: Translate): Promise<string | null> {
  try {
    const r = await commands.restartLauncher();
    if (r.status === 'ok') return null;
    return t('settings.storage.dataLocation.final.restartFailed', { error: formatError(r.error) });
  } catch (e) {
    // typedError rethrows real Error instances (a transport-level failure).
    return t('settings.storage.dataLocation.final.restartFailed', {
      error: describeStoreError(e),
    });
  }
}
