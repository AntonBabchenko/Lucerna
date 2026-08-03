// Central task registry — the single state store every progress surface
// (bottom strip, expanded panel, report modal, IntegritySection) reads from.
// Replaces three incompatible progress models: the strictly-serial
// `$lib/ops/op-queue.svelte`, components that listen to Tauri events
// directly (`PhaseStatusRow.svelte`), and state owned by whichever
// component happens to be mounted (`$lib/modpacks/modpack-update-flow.svelte`).
//
// Pure state — no IPC in this module. A later adapters module owns the
// Tauri event subscriptions and calls `start` / `upsertProgress` / `finish`;
// that split is what keeps this file unit-testable with no Tauri runtime.
//
// Same module-singleton rune idiom as `$lib/ops/op-queue.svelte` and
// `$lib/toasts/toasts.svelte`: module-level `$state`, read via getters at
// call time, reassigned (never mutated in place) so rune readers re-run.

import type { TaskDetail } from '$lib/ipc/bindings';
import type { Task, TaskLane, TaskState } from './types';

/** Finished tasks accumulate for the session and a report can hold hundreds
 *  of rows. Only this many most-recently-finished tasks keep `details` in
 *  memory; older rows keep their row but have `details` reset to `null`.
 *  Instance-scoped reports are re-readable from disk, so this loses nothing
 *  recoverable. */
const MAX_RETAINED_DETAILS = 10;

/** Fields a caller supplies to register a task. Everything else (`rate`,
 *  `state`, `caps`, `details`, `startedAt`, `finishedAt`) is derived here. */
export type StartInput = Pick<
  Task,
  'id' | 'kind' | 'scope' | 'title' | 'phase' | 'progress' | 'lane'
>;

/** A task only ever finishes into a terminal state — `queued`/`running`
 *  are not valid outcomes. */
export type FinishPatch = {
  state: Exclude<TaskState, 'queued' | 'running'>;
  details?: TaskDetail[];
};

/** Fields a progress tick may update. Kind/lane/scope are fixed at `start`. */
export type ProgressPatch = Partial<Pick<Task, 'phase' | 'progress' | 'rate'>>;

let tasks = $state<Task[]>([]);

// Plain (non-reactive) bookkeeping of the order tasks finished in, capped at
// `MAX_RETAINED_DETAILS`: the oldest id is evicted (its `details` nulled)
// the moment a new finish pushes the list past the cap. Never rendered
// directly, so it does not need to be a rune — same idiom as op-queue's
// plain `nextId` counter.
let finishedOrder: string[] = [];

/** `caps` is derived from `state`/`lane`, never stored independently — so it
 *  can never drift out of sync with the state it describes. Cancelling a
 *  RUNNING operation is out of scope for the whole feature (no backend
 *  cancellation tokens exist), so only a queued, non-modal task is
 *  cancellable; only a queued serial task is reorderable. */
function capsFor(state: TaskState, lane: TaskLane): Task['caps'] {
  return {
    cancellable: state === 'queued' && lane !== 'modal',
    reorderable: state === 'queued' && lane === 'serial',
  };
}

function sameScope(a: Task['scope'], b: Task['scope']): boolean {
  return a.instanceId === b.instanceId && a.serverId === b.serverId;
}

/** Register a new task.
 *
 * - `serial` — at most one running at a time; queues behind an
 *   already-running serial task and is promoted in `finish`.
 * - `concurrent` — starts running immediately, never queues behind
 *   anything (a game install must never wait behind a modpack import).
 * - `modal` — starts running immediately; never queues, never cancellable,
 *   never reorderable.
 */
export function start(input: StartInput): void {
  const blockedBySerial =
    input.lane === 'serial' && tasks.some((t) => t.lane === 'serial' && t.state === 'running');
  const state: TaskState = blockedBySerial ? 'queued' : 'running';

  const task: Task = {
    id: input.id,
    kind: input.kind,
    scope: input.scope,
    title: input.title,
    phase: input.phase,
    progress: input.progress,
    rate: null,
    state,
    lane: input.lane,
    caps: capsFor(state, input.lane),
    details: null,
    startedAt: Date.now(),
    finishedAt: null,
  };
  tasks = [...tasks, task];
}

/** Apply a progress tick. Silent no-op for an unknown id — late channel
 *  messages genuinely arrive after a task is gone (today's op-queue guards
 *  every progress write with the same id check, for the same reason). */
export function upsertProgress(id: string, patch: ProgressPatch): void {
  if (!tasks.some((t) => t.id === id)) return;
  tasks = tasks.map((t) => (t.id === id ? { ...t, ...patch } : t));
}

/** Move a task to a terminal state. If it was the running `serial` task,
 *  promotes the next queued `serial` task (FIFO) to `running`. Evicts the
 *  oldest finished task's `details` once more than `MAX_RETAINED_DETAILS`
 *  tasks have finished this session. */
export function finish(id: string, patch: FinishPatch): void {
  const idx = tasks.findIndex((t) => t.id === id);
  if (idx === -1) return;

  const prev = tasks[idx];
  const wasRunningSerial = prev.lane === 'serial' && prev.state === 'running';
  const finished: Task = {
    ...prev,
    state: patch.state,
    details: patch.details ?? null,
    caps: capsFor(patch.state, prev.lane),
    finishedAt: Date.now(),
  };

  let next = tasks.map((t, i) => (i === idx ? finished : t));
  if (wasRunningSerial) {
    const promoteIdx = next.findIndex((t) => t.state === 'queued');
    if (promoteIdx !== -1) {
      const promoted = next[promoteIdx];
      const running: Task = {
        ...promoted,
        state: 'running',
        caps: capsFor('running', promoted.lane),
      };
      next = next.map((t, i) => (i === promoteIdx ? running : t));
    }
  }
  tasks = next;

  finishedOrder = [...finishedOrder, id];
  if (finishedOrder.length > MAX_RETAINED_DETAILS) {
    const [evictId, ...rest] = finishedOrder;
    finishedOrder = rest;
    tasks = tasks.map((t) => (t.id === evictId ? { ...t, details: null } : t));
  }
}

/** Cancel a queued task by id. No-op if `id` is unknown, already running
 *  (cancelling a running operation is out of scope — no backend
 *  cancellation tokens exist), or `modal` (never cancellable). */
export function cancelQueued(id: string): void {
  tasks = tasks.filter((t) => !(t.id === id && t.caps.cancellable));
}

/** Move a queued `serial` task up/down by one slot among the other queued
 *  `serial` tasks. No-op at the ends, or for an unknown/non-reorderable id. */
export function moveQueued(id: string, dir: 'up' | 'down'): void {
  const reorderableIndices = tasks.reduce<number[]>((acc, t, i) => {
    if (t.caps.reorderable) acc.push(i);
    return acc;
  }, []);
  const pos = reorderableIndices.findIndex((i) => tasks[i].id === id);
  if (pos === -1) return;
  const swapWith = dir === 'up' ? pos - 1 : pos + 1;
  if (swapWith < 0 || swapWith >= reorderableIndices.length) return;

  const i = reorderableIndices[pos];
  const j = reorderableIndices[swapWith];
  const next = [...tasks];
  [next[i], next[j]] = [next[j], next[i]];
  tasks = next;
}

/** The full task list (reactive) — queued, running, and finished. */
export function taskList(): Task[] {
  return tasks;
}

/** The active (queued or running) task matching `scope`, else `null`.
 *  Deliberately excludes finished tasks: `IntegritySection` derives its
 *  button-disabling flag from this, and a lingering finished task would
 *  disable verify/repair forever. */
export function taskFor(scope: Task['scope']): Task | null {
  const isActive = (t: Task): boolean => t.state === 'queued' || t.state === 'running';
  return tasks.find((t) => isActive(t) && sameScope(t.scope, scope)) ?? null;
}

/** Test-only reset of the module singleton. */
export function __resetTasksForTest(): void {
  tasks = [];
  finishedOrder = [];
}
