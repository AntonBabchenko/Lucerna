// Once per launch: say what the startup recovery undid — a content update an
// earlier session left unfinished (a crash or a kill mid-update). The backend
// claims and undoes those before the UI can act; this only reads the outcome
// (`take_update_recovery_report`) and speaks. Spec 2026-10-04 §4.6.
//
// Pull, not an event: at startup the frontend may not be listening yet, so the
// report waits in the backend until this asks for it.
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { RecoveredUpdate } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { pushInfo, pushWarning } from '$lib/toasts/toasts.svelte';

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
  for (const r of report) {
    if (r.outcome.kind === 'restored') {
      pushInfo(tr('instance.updateRecovery.restored', { name: r.instance_name }));
    } else {
      pushWarning(
        tr('instance.updateRecovery.incomplete', {
          name: r.instance_name,
          folder: r.outcome.folder,
        }),
      );
    }
  }
}

/** Test seam: forget that the notice was given. */
export function __resetUpdateRecoveryForTest(): void {
  announced = false;
}
