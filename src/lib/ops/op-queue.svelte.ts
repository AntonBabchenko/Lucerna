// Page-level singleton store for long-running operations (integrity
// verify/repair + modpack import) as ONE strictly-serial queue: exactly one op
// runs at a time across all kinds. Generalises the former integrity-ops store
// (PR #40). Same module-singleton rune idiom as `$lib/settings/state.svelte`:
// module-level `$state`, read in templates via getters at call time.

import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { ModpackProgress, ProgressTick } from '$lib/ipc/bindings';
import { commands, events } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import type { ModpackImportRequest } from '$lib/modpacks/import-request';
import { pushActionToast, pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
import { runImport } from './import-runner';

export type IntegrityKind = 'verify' | 'repair';
export type OpKind = 'verify' | 'repair' | 'import';

export type QueuedOp =
  | { id: string; kind: IntegrityKind; instanceId: string; name: string }
  | { id: string; kind: 'import'; name: string; request: ModpackImportRequest };

export type RunningProgress =
  | { kind: IntegrityKind; filesDone: number; filesTotal: number }
  | { kind: 'import'; phase: ModpackProgress | null; bytes: ProgressTick | null };

export type RunningOp = { op: QueuedOp; progress: RunningProgress };

let running = $state<RunningOp | null>(null);
let queue = $state<QueuedOp[]>([]);
let completionTick = $state(0);
// Bumped only when an import lands a new instance (ok/partial), so the page can
// force a modpack-update sweep for the freshly-imported pack without forcing one
// on every integrity op. Kept distinct from `completionTick` to avoid that over-fire.
let importCompletionTick = $state(0);

// Monotonic id for stable keying (reorder/cancel). Imports have no instance id,
// so the queue is keyed by `id`, not `instanceId`.
let nextId = 0;
function newId(): string {
  return `op-${nextId++}`;
}

function initialProgress(op: QueuedOp): RunningProgress {
  return op.kind === 'import'
    ? { kind: 'import', phase: null, bytes: null }
    : { kind: op.kind, filesDone: 0, filesTotal: 0 };
}

// One lazy verify-progress listener (attached on first enqueue so it stays
// inert under vitest). Updates the running integrity op's file counts.
let listenerInit = false;
let unlisten: (() => void) | null = null;
function ensureListener(): void {
  if (listenerInit) return;
  listenerInit = true;
  try {
    events.verifyProgress
      .listen((e) => {
        if (
          running &&
          running.op.kind !== 'import' &&
          running.progress.kind !== 'import' &&
          e.payload.instance_id === running.op.instanceId
        ) {
          running = {
            op: running.op,
            progress: {
              kind: running.progress.kind,
              filesDone: e.payload.files_done,
              filesTotal: e.payload.files_total,
            },
          };
        }
      })
      .then((u) => {
        unlisten = u;
      })
      .catch(() => {});
  } catch {
    /* no Tauri runtime (vitest) — listener inert */
  }
}

/** Enqueue verify/repair for an instance. Dedupe: an instance that already has
 *  a running or queued integrity op is a no-op. Kicks the drain loop. */
export function enqueueIntegrity(instanceId: string, name: string, kind: IntegrityKind): void {
  ensureListener();
  if (running && running.op.kind !== 'import' && running.op.instanceId === instanceId) return;
  if (queue.some((q) => q.kind !== 'import' && q.instanceId === instanceId)) return;
  queue = [...queue, { id: newId(), kind, instanceId, name }];
  void processNext();
}

async function processNext(): Promise<void> {
  if (running || queue.length === 0) return;
  const op = queue[0];
  queue = queue.slice(1);
  running = { op, progress: initialProgress(op) };

  if (op.kind === 'import') {
    await runImportOp(op);
  } else {
    await runIntegrity(op);
  }

  completionTick += 1;
  running = null;
  void processNext();
}

async function runIntegrity(op: Extract<QueuedOp, { kind: IntegrityKind }>): Promise<void> {
  const res =
    op.kind === 'verify'
      ? await commands.verifyInstance(op.instanceId)
      : await commands.repairInstance(op.instanceId);

  const tr = get(t);
  if (res.status === 'ok') {
    const report = res.data;
    if (op.kind === 'verify') {
      if (report.healthy) {
        pushSuccess(tr('instance.integrity.toastVerifyOk', { name: op.name }));
      } else {
        pushWarning(
          tr('instance.integrity.toastVerifyProblems', {
            name: op.name,
            count: report.problems.length,
          }),
        );
      }
    } else if (report.healthy) {
      pushSuccess(tr('instance.integrity.toastRepaired', { name: op.name }));
    } else {
      pushWarning(
        tr('instance.integrity.toastRepairPartial', {
          name: op.name,
          count: report.problems.length,
        }),
      );
    }
  } else {
    pushWarning(
      tr('instance.integrity.toastError', { name: op.name, error: formatError(res.error) }),
    );
  }
}

function importKey(r: ModpackImportRequest): string {
  return r.path || `${r.source}:${r.projectId}:${r.versionId}`;
}

export function enqueueImport(name: string, request: ModpackImportRequest): void {
  ensureListener();
  const key = importKey(request);
  if (running && running.op.kind === 'import' && importKey(running.op.request) === key) return;
  if (queue.some((q) => q.kind === 'import' && importKey(q.request) === key)) return;
  queue = [...queue, { id: newId(), kind: 'import', name, request }];
  void processNext();
}

async function selectInstance(id: string): Promise<void> {
  if (!id) return;
  await commands.setActiveInstance(id);
  completionTick += 1; // page effect re-reads the active instance
}

async function runImportOp(op: Extract<QueuedOp, { kind: 'import' }>): Promise<void> {
  const outcome = await runImport(op.request, (phase, bytes) => {
    if (running && running.op.id === op.id) {
      running = { op, progress: { kind: 'import', phase, bytes } };
    }
  });
  const tr = get(t);
  if (outcome.status === 'ok') {
    const lines =
      outcome.skipped.length > 0
        ? outcome.skipped.map((s) =>
            tr('page.modpackImport.skippedOverrideLine', {
              name: s.path.split('/').pop() ?? s.path,
              mb: Math.round((s.size ?? 0) / (1024 * 1024)),
            }),
          )
        : [];
    const title =
      outcome.skipped.length > 0
        ? tr('page.modpackImport.importedSkipped', {
            name: outcome.name,
            count: outcome.skipped.length,
          })
        : tr('page.modpackImport.imported', { name: outcome.name });
    const id = outcome.instanceId;
    pushActionToast(
      'success',
      title,
      { label: tr('ops.openInstance'), run: () => void selectInstance(id) },
      lines,
    );
    importCompletionTick += 1;
  } else if (outcome.status === 'partial') {
    pushWarning(
      tr('page.modpackImport.partialFailure', { count: outcome.failed.length }),
      outcome.failed,
    );
    importCompletionTick += 1;
  } else {
    pushWarning(tr('page.modpackImport.failed'), [outcome.message]);
  }
}

/** Cancel a QUEUED op by id (no-op if it is the running op or unknown). */
export function cancelQueued(id: string): void {
  queue = queue.filter((q) => q.id !== id);
}

/** Move a QUEUED op up/down by one slot (no-op at the ends or if unknown). */
export function moveQueued(id: string, dir: 'up' | 'down'): void {
  const i = queue.findIndex((q) => q.id === id);
  if (i === -1) return;
  const j = dir === 'up' ? i - 1 : i + 1;
  if (j < 0 || j >= queue.length) return;
  const next = [...queue];
  [next[i], next[j]] = [next[j], next[i]];
  queue = next;
}

/** Live integrity phase for an instance, for IntegritySection to render. */
export function opStatusFor(instanceId: string): {
  phase: 'running' | 'queued';
  kind: IntegrityKind;
  filesDone: number;
  filesTotal: number;
} | null {
  if (running && running.op.kind !== 'import' && running.op.instanceId === instanceId) {
    const p = running.progress;
    return {
      phase: 'running',
      kind: running.op.kind,
      filesDone: p.kind === 'import' ? 0 : p.filesDone,
      filesTotal: p.kind === 'import' ? 0 : p.filesTotal,
    };
  }
  const queued = queue.find((q) => q.kind !== 'import' && q.instanceId === instanceId);
  if (queued && queued.kind !== 'import') {
    return { phase: 'queued', kind: queued.kind, filesDone: 0, filesTotal: 0 };
  }
  return null;
}

/** Reactive getters (read module `$state` at call time). */
export function opRunning(): RunningOp | null {
  return running;
}
export function opQueue(): QueuedOp[] {
  return queue;
}
export function opCompletionTick(): number {
  return completionTick;
}
/** Bumps when an import lands a new instance (ok/partial); drives the page's
 *  forced post-import modpack-update sweep. */
export function opImportCompletionTick(): number {
  return importCompletionTick;
}

/** Test-only reset of the module singleton. */
export function __resetOpQueueForTest(): void {
  running = null;
  queue = [];
  completionTick = 0;
  importCompletionTick = 0;
  nextId = 0;
  unlisten?.();
  unlisten = null;
  listenerInit = false;
}
