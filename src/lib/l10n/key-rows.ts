// Pure view logic for the per-namespace key table. No DOM, no IPC — unit-tested
// in isolation, same split as coverage.ts.
import type { KeyRow, KeyState, Origin } from '$lib/ipc/bindings';

/** The state filter chips in KeyTable. 'translated' folds together the two
 *  states that need no attention (`from_mod` and `ok`) — the user thinks in
 *  terms of "done" vs. the three ways a key still needs work, not in terms
 *  of the five raw backend states. */
export type KeyFilter = 'all' | 'missing' | 'stale' | 'orphan' | 'translated';

/** What the user currently sees in game for one key: their override if they
 *  have one, else the mod's own translation, else nothing — matches the
 *  precedence Minecraft's resource-pack stack actually applies. */
export function displayValue(row: KeyRow): string {
  return row.overrideValue ?? row.modValue ?? '';
}

// States that fold into "translated" — nothing for the user to do.
const TRANSLATED_STATES: ReadonlySet<KeyState> = new Set(['from_mod', 'ok']);

function matchesFilter(row: KeyRow, filter: KeyFilter): boolean {
  switch (filter) {
    case 'all':
      return true;
    case 'translated':
      return TRANSLATED_STATES.has(row.state);
    case 'missing':
      return row.state === 'missing';
    case 'stale':
      return row.state === 'stale';
    case 'orphan':
      return row.state === 'orphan';
  }
}

/** Apply the state filter, then the search term. Search matches the key or
 *  the English source — the two things a user knows going in — never the
 *  translated value itself: that may be in a script the user can't even
 *  read, which is the entire reason they're translating it. */
export function filterRows(rows: KeyRow[], search: string, filter: KeyFilter): KeyRow[] {
  const q = search.trim().toLowerCase();
  return rows.filter((row) => {
    if (!matchesFilter(row, filter)) return false;
    if (!q) return true;
    return row.key.toLowerCase().includes(q) || row.sourceEn.toLowerCase().includes(q);
  });
}

/** The origin filter chips in KeyTable — a SECOND axis over [`KeyFilter`],
 *  not more options on it. A key has a state *and*, once it is overridden, an
 *  origin; folding the two into one single-select group would make
 *  "untranslated" and "machine-written" mutually exclusive, which they are
 *  not. 'all' means "don't filter on this axis at all". */
export type OriginFilter = 'all' | Origin;

/** Narrow rows to one override origin. A key with no override has no origin
 *  to attribute, so it appears under 'all' and under neither of the other
 *  two — the same reason `null` is a distinct value on the row itself. */
export function filterByOrigin(rows: KeyRow[], origin: OriginFilter): KeyRow[] {
  if (origin === 'all') return rows;
  return rows.filter((row) => row.origin === origin);
}

/** How many rows carry each override origin, in one pass. The machine count
 *  also drives bulk revert, which has to be able to say how many entries it
 *  is about to drop *before* the user confirms it. */
export function countOrigins(rows: KeyRow[]): { manual: number; machine: number } {
  const counts = { manual: 0, machine: 0 };
  for (const row of rows) {
    if (row.origin === 'manual') counts.manual++;
    else if (row.origin === 'machine') counts.machine++;
  }
  return counts;
}

export type FilterCounts = {
  all: number;
  translated: number;
  stale: number;
  orphan: number;
  missing: number;
};

/** Per-bucket counts for the filter chips, computed over the full row set —
 *  independent of the search term and the currently active filter, so a
 *  chip's count never shifts under the user while they're typing. */
export function countKeyStates(rows: KeyRow[]): FilterCounts {
  const counts: FilterCounts = { all: rows.length, translated: 0, stale: 0, orphan: 0, missing: 0 };
  for (const row of rows) {
    if (TRANSLATED_STATES.has(row.state)) counts.translated++;
    else if (row.state === 'stale') counts.stale++;
    else if (row.state === 'orphan') counts.orphan++;
    else if (row.state === 'missing') counts.missing++;
  }
  return counts;
}
