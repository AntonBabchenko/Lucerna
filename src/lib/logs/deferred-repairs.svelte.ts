// Session-scoped queue of repairs deferred because a game is running. The
// backend `execute_repair` rejects with InstanceBusy while Minecraft holds the
// instance's files open (Windows), so instead of failing we hold the action and
// run it on the next `processExited`. Same module-singleton rune idiom as
// repair-ops.svelte / op-queue.svelte. Lost on launcher restart (session-only).

import { SvelteSet } from 'svelte/reactivity';
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { RepairChoice } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';

type DeferredRepair = {
  id: string;
  instanceId: string;
  sha1: string | null;
  label: string;
  choice: RepairChoice;
};

let queue = $state<DeferredRepair[]>([]);
const completed = new SvelteSet<string>();

// Instance-level repairs (no specific mod) use an explicit sentinel so their key
// can't collide with a mod whose sha1 is somehow empty.
const key = (instanceId: string, sha1: string | null) => `${instanceId}::${sha1 ?? '__instance__'}`;
let seq = 0;

export function isDeferred(instanceId: string, sha1: string | null): boolean {
  return queue.some((q) => q.instanceId === instanceId && q.sha1 === sha1);
}
export function isCompleted(instanceId: string, sha1: string | null): boolean {
  return completed.has(key(instanceId, sha1));
}
export function deferredCount(): number {
  return queue.length;
}

async function runOne(e: DeferredRepair): Promise<boolean> {
  const tr = get(t);
  const res = await commands.executeRepair(e.instanceId, e.choice);
  if (res.status === 'ok') {
    completed.add(key(e.instanceId, e.sha1));
    pushSuccess(tr('logs.repair.toastDone', { name: e.label }));
    return true;
  }
  pushWarning(tr('logs.repair.toastError', { name: e.label, error: formatError(res.error) }));
  return false;
}

/** Run the repair now, or — if a game is running — queue it for after exit. */
export async function deferOrRunRepair(
  running: boolean,
  entry: { instanceId: string; sha1: string | null; label: string; choice: RepairChoice },
): Promise<{ deferred: boolean; ok: boolean }> {
  if (running) {
    queue = [...queue, { id: `d${seq++}`, ...entry }];
    pushSuccess(get(t)('logs.repair.queuedToast', { name: entry.label }));
    return { deferred: true, ok: true };
  }
  const ok = await runOne({ id: `d${seq++}`, ...entry });
  return { deferred: false, ok };
}

/** Test-only: clear all session state so tests don't leak into each other. */
export function __resetDeferredRepairsForTest(): void {
  queue = [];
  completed.clear();
  seq = 0;
}

/** Apply every queued repair (serially). Call on processExited. */
export async function drainDeferredRepairs(): Promise<void> {
  if (queue.length === 0) return;
  const pending = queue;
  queue = [];
  for (const e of pending) {
    await runOne(e);
  }
}
