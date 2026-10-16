// Once per launch: say what the startup recovery undid — a content update an
// earlier session left unfinished (a crash or a kill mid-update). The backend
// claims and undoes those before the UI can act; this only reads the outcome
// (`take_update_recovery_report`) and speaks. Spec 2026-10-04 §4.6.
//
// Pull, not an event: at startup the frontend may not be listening yet, so the
// report waits in the backend until this asks for it.
import { get } from 'svelte/store';
import { type Translate, t } from '$lib/i18n';
import type { RecoveredUpdate } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { pushActionToast, pushInfo, pushWarning } from '$lib/toasts/toasts.svelte';

/** Injectable dependencies (tests pass fakes); production uses the backend. */
export interface UpdateRecoveryDeps {
  take?: () => Promise<RecoveredUpdate[] | null>;
}

let announced = false;

async function takeFromBackend(): Promise<RecoveredUpdate[] | null> {
  const r = await commands.takeUpdateRecoveryReport();
  return r.status === 'ok' ? r.data : null;
}

/** One notice per recovered update; never throws, never repeats. */
export async function announceUpdateRecovery(deps: UpdateRecoveryDeps = {}): Promise<void> {
  if (announced) return;
  announced = true;
  const take = deps.take ?? takeFromBackend;
  let report: RecoveredUpdate[] | null;
  try {
    report = await take();
  } catch {
    // The bridge failed: nothing to say here — the backend log has the
    // recovery's own lines.
    return;
  }
  if (report === null) return;
  const tr = get(t);
  for (const r of report) speak(tr, r);
}

/** The notice for one entry. A mod-data copy runs through the same
 *  transaction as an update, so the wording names which one it was — and a
 *  token whose record could not be read names neither (`unknown`). */
function speak(tr: Translate, r: RecoveredUpdate): void {
  const name = r.instance_name;
  const op = r.operation;
  switch (r.outcome.kind) {
    case 'restored':
      pushInfo(
        op.kind === 'data_copy'
          ? tr('instance.updateRecovery.dataCopyRestored', { name, source: op.source })
          : op.kind === 'update'
            ? tr('instance.updateRecovery.restored', { name })
            : tr('instance.updateRecovery.unknownRestored', { name }),
      );
      return;
    case 'incomplete':
      pushWarning(
        tr(
          op.kind === 'data_copy'
            ? 'instance.updateRecovery.dataCopyIncomplete'
            : op.kind === 'update'
              ? 'instance.updateRecovery.incomplete'
              : 'instance.updateRecovery.unknownIncomplete',
          { name, folder: r.outcome.folder },
        ),
      );
      return;
    // Only a mod-data copy keeps the files it replaces. The folder's name, not
    // its path, with a way to open it.
    case 'kept_aside': {
      const folder = folderName(r.outcome.folder);
      pushActionToast('info', tr('instance.updateRecovery.keptAside', { name, folder }), {
        label: tr('instance.updateRecovery.openFolder'),
        run: () => void openKept(tr, r.instance_id, folder),
      });
      return;
    }
    case 'kept_in_place':
      pushWarning(tr('instance.updateRecovery.keptInPlace', { name, folder: r.outcome.folder }));
      return;
  }
}

/** The last segment of a path the backend reported, whichever separator it uses. */
function folderName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

async function openKept(tr: Translate, instanceId: string, folder: string): Promise<void> {
  const r = await commands.openKeptFolder(instanceId, folder);
  if (r.status === 'error') {
    pushWarning(tr('instance.updateRecovery.openFolderFailed'), [formatError(r.error)]);
  }
}

/** Test seam: forget that the notice was given. */
export function __resetUpdateRecoveryForTest(): void {
  announced = false;
}
