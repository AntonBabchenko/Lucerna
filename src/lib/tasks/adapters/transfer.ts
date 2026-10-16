// Task-registry adapter for a mod-data copy: one serial task scoped to the
// TARGET instance (the one being written), with the runner's progress mapped
// onto the row. The backend also claims the source; the row names the target.
import type { TransferRequest, TransferRunOutcome } from '$lib/instances/transfer/transfer-runner';
import { runTransfer } from '$lib/instances/transfer/transfer-runner';
import { finish, start, TaskCancelledError, upsertProgress } from '../registry.svelte';

export type TransferTaskOutcome = TransferRunOutcome | { status: 'cancelled' };

export async function transferData(
  title: string,
  request: TransferRequest,
): Promise<TransferTaskOutcome> {
  const id = `transfer-${crypto.randomUUID()}`;
  try {
    await start({
      id,
      kind: 'transfer',
      scope: { instanceId: request.targetId },
      title,
      phase: null,
      progress: null,
      lane: 'serial',
    });
    const outcome = await runTransfer(request, (p) => {
      if (p === null) {
        upsertProgress(id, { phase: null, progress: null });
        return;
      }
      upsertProgress(id, {
        phase: p.phase,
        progress: { current: p.current, total: p.total, unit: 'files' },
      });
    });
    finish(id, { state: outcome.status === 'ok' ? 'ok' : 'failed' });
    return outcome;
  } catch (e) {
    if (e instanceof TaskCancelledError) {
      finish(id, { state: 'cancelled' });
      return { status: 'cancelled' };
    }
    finish(id, { state: 'failed' });
    return { status: 'error', message: e instanceof Error ? e.message : String(e) };
  }
}
