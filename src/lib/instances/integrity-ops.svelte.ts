// Page-level singleton store for integrity verify/repair as a background
// operation. Ownership moved out of IntegritySection (which now only
// observes) so an op survives the Manage modal closing / tab switching,
// surfaces in a global progress view, reports via toast, and — critically —
// runs ONE AT A TIME through a frontend queue (verify/repair write to the
// shared assets/ + libraries/ dirs, so concurrency risks file races).
//
// Same module-singleton rune idiom as `$lib/settings/state.svelte` and
// `$lib/toasts/toasts.svelte`: module-level `$state`, read in templates via
// exported getter functions (read at call time → stays reactive in Svelte 5).

import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { commands, events } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';

export type IntegrityKind = 'verify' | 'repair';

export type RunningOp = {
  instanceId: string;
  name: string;
  kind: IntegrityKind;
  filesDone: number;
  filesTotal: number;
};

type QueuedOp = { instanceId: string; name: string; kind: IntegrityKind };

let running = $state<RunningOp | null>(null);
let queue = $state<QueuedOp[]>([]);
let completionTick = $state(0);

// One module-level progress listener, attached lazily on first enqueue so it
// stays inert under vitest (no Tauri runtime → `events.verifyProgress.listen`
// throws / rejects, which we swallow).
let listenerInit = false;
function ensureListener(): void {
  if (listenerInit) return;
  listenerInit = true;
  try {
    events.verifyProgress
      .listen((e) => {
        if (running && e.payload.instance_id === running.instanceId) {
          running = {
            ...running,
            filesDone: e.payload.files_done,
            filesTotal: e.payload.files_total,
          };
        }
      })
      .catch(() => {});
  } catch {
    /* no Tauri runtime (vitest) — listener inert */
  }
}

/**
 * Enqueue a verify/repair for an instance. Dedupe: a second request for an
 * instance that already has a running or queued op is a no-op. Kicks the
 * drain loop.
 */
export function enqueueIntegrity(instanceId: string, name: string, kind: IntegrityKind): void {
  ensureListener();
  if (running?.instanceId === instanceId) return;
  if (queue.some((q) => q.instanceId === instanceId)) return;
  queue = [...queue, { instanceId, name, kind }];
  void processNext();
}

async function processNext(): Promise<void> {
  if (running || queue.length === 0) return;
  const next = queue[0];
  queue = queue.slice(1);
  running = { ...next, filesDone: 0, filesTotal: 0 };

  const res =
    next.kind === 'verify'
      ? await commands.verifyInstance(next.instanceId)
      : await commands.repairInstance(next.instanceId);

  const tr = get(t);
  if (res.status === 'ok') {
    const report = res.data;
    if (next.kind === 'verify') {
      if (report.healthy) {
        pushSuccess(tr('instance.integrity.toastVerifyOk', { name: next.name }));
      } else {
        pushWarning(
          tr('instance.integrity.toastVerifyProblems', {
            name: next.name,
            count: report.problems.length,
          }),
        );
      }
    } else if (report.healthy) {
      pushSuccess(tr('instance.integrity.toastRepaired', { name: next.name }));
    } else {
      pushWarning(
        tr('instance.integrity.toastRepairPartial', {
          name: next.name,
          count: report.problems.length,
        }),
      );
    }
  } else {
    pushWarning(
      tr('instance.integrity.toastError', { name: next.name, error: formatError(res.error) }),
    );
  }

  completionTick += 1;
  running = null;
  void processNext();
}

/** Live phase for an instance, for the section to render. */
export function integrityStatusFor(
  instanceId: string,
): { phase: 'running' | 'queued'; filesDone: number; filesTotal: number } | null {
  if (running?.instanceId === instanceId) {
    return { phase: 'running', filesDone: running.filesDone, filesTotal: running.filesTotal };
  }
  if (queue.some((q) => q.instanceId === instanceId)) {
    return { phase: 'queued', filesDone: 0, filesTotal: 0 };
  }
  return null;
}

/** Reactive getters for views (read module `$state` at call time). */
export function integrityRunning(): RunningOp | null {
  return running;
}

export function integrityQueueLength(): number {
  return queue.length;
}

export function integrityCompletionTick(): number {
  return completionTick;
}

/**
 * Test-only reset of the module singleton. The store is a singleton shared
 * across the whole app, so vitest needs a way to clear state between cases.
 * Not used in production paths.
 */
export function __resetIntegrityOpsForTest(): void {
  running = null;
  queue = [];
  completionTick = 0;
  listenerInit = false;
}
