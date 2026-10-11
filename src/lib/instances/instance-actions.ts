// Per-instance actions the page runs for the sidebar's row menu. Their failures
// are toasts: the menu can be used from any tab and in compact mode, and the
// page's inline banners render on the Overview only.
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { commands } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { pushWarning } from '$lib/toasts/toasts.svelte';

export async function openInstanceFolderReporting(id: string): Promise<void> {
  const r = await commands.openInstanceFolder(id);
  if (r.status === 'error') {
    pushWarning(get(t)('instance.manage.openFolderFailed'), [formatError(r.error)]);
  }
}

export async function openModsFolderReporting(id: string): Promise<void> {
  const r = await commands.openModsFolder(id);
  if (r.status === 'error') {
    pushWarning(get(t)('instance.manage.openModsFolderFailed'), [formatError(r.error)]);
  }
}

/** An activation the user asked for from a surface with no error slot of its
 *  own (the sidebar list, its row menu, the create dialog). */
export function reportActivationFailure(name: string, message: string): void {
  pushWarning(get(t)('instance.settings.activateFailed', { name }), [message]);
}
