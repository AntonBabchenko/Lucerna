// Page-level singleton store for one-click crash auto-repair execution.
// Same module-singleton rune idiom as `$lib/instances/integrity-ops.svelte`:
// module-level `$state`, read in templates via exported getter functions.
//
// Unlike integrity ops, a repair is a single discrete action (raise heap /
// reinstall loader / disable mod / reinstall mod) with no progress event,
// so this store has no progress listener — just enqueue -> run -> toast.
// One-at-a-time PER INSTANCE: a second request for an instance already
// running or queued is ignored.

import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { RepairChoice } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';

type RunningRepair = { instanceId: string; name: string };
type QueuedRepair = { instanceId: string; name: string; choice: RepairChoice };

let running = $state<RunningRepair | null>(null);
let queue = $state<QueuedRepair[]>([]);
let completionTick = $state(0);

/**
 * Enqueue a repair for an instance. Dedupe: a request for an instance that
 * already has a running or queued op is a no-op (resolves immediately). When
 * the queue was idle, the returned promise awaits the synchronous drain it
 * kicks off — which, in the single-instance caller, means it resolves once
 * this op has run. With concurrent ops for other instances the drain may
 * return before every queued op finishes; callers needing per-op completion
 * should observe `repairCompletionTick`/`repairRunningFor` instead.
 */
export async function enqueueRepair(
  instanceId: string,
  name: string,
  choice: RepairChoice,
): Promise<void> {
  if (running?.instanceId === instanceId) return;
  if (queue.some((q) => q.instanceId === instanceId)) return;
  queue = [...queue, { instanceId, name, choice }];
  await processNext();
}

async function processNext(): Promise<void> {
  if (running || queue.length === 0) return;
  const next = queue[0];
  queue = queue.slice(1);
  running = { instanceId: next.instanceId, name: next.name };

  const res = await commands.executeRepair(next.instanceId, next.choice);
  const tr = get(t);
  if (res.status === 'ok') {
    pushSuccess(tr('logs.repair.toastDone', { name: next.name }));
  } else {
    pushWarning(tr('logs.repair.toastError', { name: next.name, error: formatError(res.error) }));
  }

  completionTick += 1;
  running = null;
  await processNext();
}

/** Reactive getter: the running repair for an instance, or null. */
export function repairRunningFor(instanceId: string): RunningRepair | null {
  return running?.instanceId === instanceId ? running : null;
}

export function repairCompletionTick(): number {
  return completionTick;
}

/** Test-only reset of the module singleton. */
export function __resetRepairOpsForTest(): void {
  running = null;
  queue = [];
  completionTick = 0;
}
