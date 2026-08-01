// Pure view logic for the per-namespace key table. No DOM, no IPC — unit-tested
// in isolation, same split as coverage.ts.
import type { KeyRow, KeyState } from '$lib/ipc/bindings';

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
