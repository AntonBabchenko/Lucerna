import { commands, type ModpackUpdateStatus } from '$lib/ipc/bindings';

// Single source of truth for "does instance X have a modpack update?".
// All four surfaces (Imported cards, sidebar switcher, the global count
// badge, and the Overview attention panel) read from this one store, so a
// sweep hits the network once and every surface reflects it.
//
// Client-only `checking` augments the backend status set; everything else
// mirrors the Rust `ModpackUpdateStatus` enum 1:1.
type Status = ModpackUpdateStatus | { kind: 'checking' };

// Re-check no more than once per window unless a caller forces it. The app
// also re-sweeps on each launch (cheap, and keeps offline users from seeing
// stale-forever badges within a session).
const TTL_MS = 6 * 60 * 60 * 1000;

let statuses = $state<Map<string, Status>>(new Map());
let lastSweepAt = $state<number | null>(null);

const updateCount = $derived(
  [...statuses.values()].filter((s) => s.kind === 'update_available').length,
);

async function sweep(instanceIds: string[], opts?: { force?: boolean }) {
  if (instanceIds.length === 0) return;
  const now = Date.now();
  if (!opts?.force && lastSweepAt !== null && now - lastSweepAt < TTL_MS) return;

  // Snapshot each requested instance's status BEFORE overwriting with
  // 'checking' so an error can restore the exact prior entry (e.g. a standing
  // 'update_available' must survive a failed re-check, not be destroyed).
  const prior = new Map<string, Status | undefined>();
  for (const id of instanceIds) prior.set(id, statuses.get(id));

  const next = new Map(statuses);
  for (const id of instanceIds) next.set(id, { kind: 'checking' });
  statuses = next;

  const r = await commands.modpacksCheckUpdates(instanceIds);
  // Errors are non-fatal: a failed sweep restores each requested instance's
  // prior status and is simply retried on the next trigger. Badges never block
  // the UI. Only ids that had no prior entry are dropped.
  if (r.status === 'error') {
    const reverted = new Map(statuses);
    for (const id of instanceIds) {
      // Only touch entries we set to 'checking' — a concurrent write may have
      // already replaced ours with a fresh verdict, which we must not clobber.
      if (reverted.get(id)?.kind !== 'checking') continue;
      const before = prior.get(id);
      if (before === undefined) reverted.delete(id);
      else reverted.set(id, before);
    }
    statuses = reverted;
    return;
  }
  const merged = new Map(statuses);
  for (const row of r.data) merged.set(row.instance_id, row.status);
  statuses = merged;
  lastSweepAt = now;
}

function invalidate(id: string) {
  if (!statuses.has(id)) return;
  const next = new Map(statuses);
  next.delete(id);
  statuses = next;
}

export const modpackUpdates = {
  get updateCount() {
    return updateCount;
  },
  statusFor(id: string): Status | undefined {
    return statuses.get(id);
  },
  hasUpdate(id: string): boolean {
    return statuses.get(id)?.kind === 'update_available';
  },
  sweep,
  invalidate,
  // Test-only: clear all state between cases.
  reset() {
    statuses = new Map();
    lastSweepAt = null;
  },
};
