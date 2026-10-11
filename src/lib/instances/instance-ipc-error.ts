// Error wording for the instance editors: the Settings tab's sections and the
// create dialog. Shorter than the shared formatter for the name and folder
// cases — on a surface that is all about one instance, the "Instance" prefix
// `formatError` adds is redundant. Everything else delegates to `formatError`,
// so no IPC variant leaks raw JSON (e.g. openInstanceFolder returning Io).
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { Error as IpcError } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';

export function instanceIpcErrorMessage(e: IpcError): string {
  if (e.kind === 'instance_name_empty') return get(t)('instance.error.nameEmpty');
  if (e.kind === 'instance_name_too_long')
    return get(t)('instance.error.nameTooLong', { actual: e.actual, max: e.max });
  if (e.kind === 'instance_dir_name_empty') return get(t)('instance.error.dirNameEmpty');
  if (e.kind === 'instance_dir_name_taken')
    return get(t)('instance.error.dirNameTaken', { name: e.name });
  if (e.kind === 'instance_dir_name_reserved')
    return get(t)('instance.error.dirNameReserved', { name: e.name });
  if (e.kind === 'instance_dir_locked') return get(t)('instance.error.dirLocked', { name: e.name });
  if (e.kind === 'path_not_launchable') return get(t)('instance.error.pathNotLaunchable');
  return formatError(e);
}
