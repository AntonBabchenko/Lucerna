/**
 * The mod-update check of each profile, held ONCE for the whole app (spec 2026-09-28 §5.5, plan
 * A18): the Installed tab's badges and «проверено …» and the Overview's «Обновлений: N» read the
 * same copy, so they cannot disagree, and a check run on the Installed tab reaches the Overview at
 * once. It replaced the session LRU `update-check-cache.ts`.
 *
 * - `loadStoredUpdateCheck` reads the persisted check back (`mods_last_update_check`: rows only for
 *   jars still installed and projects not on hold). A file the backend cannot read it logs and
 *   answers as none; the command's own error and a failed bridge read the same way here — "not
 *   checked", no stale badges (§9).
 * - Only the read started LAST for a profile commits, and only if nothing was written here for it
 *   since it started (a fresh check, the rows an update or a hold dropped): an earlier read carries
 *   a file those have already replaced.
 * - `checkForUpdates` runs `mods_check_updates` — one at a time per profile; a second call joins
 *   the running one — and commits its answer, which the backend persisted before it answered. If
 *   that write failed (logged there), the file still holds an OLDER check, or none: the fresh
 *   answer then stays for the session (§5.5), and a stored check older than it is not taken.
 *   Known limit: while that lasts, a mod removed some other way keeps its row here until the next
 *   check (the rows of the tab's own updates and holds are dropped as they happen).
 *
 * Every decision reads PLAIN module state; `shown` / `running` are the reactive copies readers
 * render, replaced whole on each change (the mod-ops pattern: during a teardown Svelte serves
 * pre-batch `$state`).
 */
import {
  commands,
  type Error as IpcError,
  type ModUpdateCheck,
  type StoredUpdateCheck,
} from '$lib/ipc/bindings';

/** A profile's last check: when it ran, and one row per mod it judged. */
export type StoredCheck = {
  readonly checkedAtMs: number;
  readonly results: readonly ModUpdateCheck[];
};

type CheckAnswer = { status: 'ok'; data: ModUpdateCheck[] } | { status: 'error'; error: IpcError };

type Slot = {
  /** The ticket of the read started last. */
  reads: number;
  /** Bumped by every write made here — a read that left before one never commits. */
  writes: number;
  /** Unix seconds when this session's own check started; an older stored check is not newer. */
  freshSince: number | null;
};

const slots = new Map<string, Slot>();
const inFlight = new Map<string, Promise<CheckAnswer>>();
/** Bumped by the test reset: a check started before it commits nothing and leaves no trace. */
let epoch = 0;
let checks: ReadonlyMap<string, StoredCheck> = new Map();
let shown = $state.raw<ReadonlyMap<string, StoredCheck>>(checks);
let running = $state.raw<ReadonlySet<string>>(new Set());

function slotOf(instanceId: string): Slot {
  let slot = slots.get(instanceId);
  if (!slot) {
    slot = { reads: 0, writes: 0, freshSince: null };
    slots.set(instanceId, slot);
  }
  return slot;
}

function put(instanceId: string, check: StoredCheck | null): void {
  const next = new Map(checks);
  if (check) next.set(instanceId, check);
  else next.delete(instanceId);
  checks = next;
  shown = next;
}

function publishRunning(): void {
  running = new Set(inFlight.keys());
}

/** The last check of `instanceId`, or null: never checked, not read yet, or unreadable. Reactive. */
export function storedUpdateCheck(instanceId: string | null): StoredCheck | null {
  return instanceId ? (shown.get(instanceId) ?? null) : null;
}

/** Pending updates in that check; null when there is none — "not known", never a reassuring 0. */
export function pendingUpdateCount(instanceId: string | null): number | null {
  const check = storedUpdateCheck(instanceId);
  return check ? check.results.filter((c) => c.state.kind === 'update_available').length : null;
}

/** A check for `instanceId` is running. Reactive. */
export function isCheckingUpdates(instanceId: string | null): boolean {
  return instanceId !== null && running.has(instanceId);
}

/** Read the persisted check of `instanceId` back. Never throws. */
export async function loadStoredUpdateCheck(instanceId: string): Promise<void> {
  const slot = slotOf(instanceId);
  const ticket = ++slot.reads;
  const writes = slot.writes;
  let answer: StoredUpdateCheck | null;
  try {
    const r = await commands.modsLastUpdateCheck(instanceId);
    answer = r.status === 'ok' ? r.data : null;
  } catch {
    // The bridge failed: there is no answer to read, which is "not checked" as well.
    answer = null;
  }
  if (slots.get(instanceId) !== slot || slot.reads !== ticket || slot.writes !== writes) return;
  // This session's own check is newer than the file: its save failed (§5.5).
  if (slot.freshSince !== null && (answer === null || answer.checked_at_secs < slot.freshSince)) {
    return;
  }
  put(
    instanceId,
    answer ? { checkedAtMs: answer.checked_at_secs * 1000, results: answer.results } : null,
  );
}

/**
 * Check `instanceId` for updates now. A call while one runs for that profile joins it. Resolves
 * with the command's answer — committed here when it is one — and rejects only when the bridge
 * itself failed.
 */
export function checkForUpdates(instanceId: string): Promise<CheckAnswer> {
  const pending = inFlight.get(instanceId);
  if (pending) return pending;
  // Recorded BEFORE the check starts: a call that fails at once must find its entry to remove.
  let start: (check: Promise<CheckAnswer>) => void = () => {};
  const run = new Promise<CheckAnswer>((resolve) => {
    start = resolve;
  });
  inFlight.set(instanceId, run);
  publishRunning();
  start(runCheck(instanceId, epoch));
  return run;
}

async function runCheck(instanceId: string, started: number): Promise<CheckAnswer> {
  const startedAt = Date.now();
  try {
    const r = await commands.modsCheckUpdates(instanceId);
    if (r.status === 'ok' && epoch === started) commitFresh(instanceId, r.data, startedAt);
    return r;
  } finally {
    if (epoch === started) {
      inFlight.delete(instanceId);
      publishRunning();
    }
  }
}

function commitFresh(instanceId: string, results: ModUpdateCheck[], startedAt: number): void {
  const slot = slotOf(instanceId);
  slot.writes += 1;
  slot.freshSince = Math.floor(startedAt / 1000);
  put(instanceId, { checkedAtMs: Date.now(), results });
}

/**
 * Drop the rows `drop` picks — jars an update replaced, a project just held — as the next stored
 * read would. A read already under way is not taken: it was answered before the change.
 */
export function dropUpdateRows(instanceId: string, drop: (c: ModUpdateCheck) => boolean): void {
  slotOf(instanceId).writes += 1;
  const check = checks.get(instanceId);
  if (!check) return;
  const results = check.results.filter((c) => !drop(c));
  if (results.length !== check.results.length) put(instanceId, { ...check, results });
}

export function __resetUpdateCheckStoreForTests(): void {
  // Reads and checks still under way belong to the slots and runs dropped here: they commit
  // nothing.
  epoch += 1;
  slots.clear();
  inFlight.clear();
  publishRunning();
  checks = new Map();
  shown = checks;
}
