// Failure toast for a mod install: the mod's name in the title, the localized
// cause as the detail line, and a Retry action. The backend made the install
// atomic (partial installs are rolled back), so retrying is always safe — the
// caller re-runs the same command with the same arguments.
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { Error as IpcError } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { pushActionToast } from '$lib/toasts/toasts.svelte';

export function installFailureToast(modName: string, err: IpcError, retry: () => void): number {
  const tr = get(t);
  return pushActionToast(
    'warning',
    tr('mods.browse.toastInstallFailedWithMod', { name: modName }),
    { label: tr('mods.browse.toastRetry'), run: retry },
    [formatError(err)],
  );
}
