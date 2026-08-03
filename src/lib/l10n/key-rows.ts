// Pure view logic for the per-namespace key table. No DOM, no IPC — unit-tested
// in isolation, same split as coverage.ts.
import type { KeyRow, KeyState } from '$lib/ipc/bindings';

/** One entry in the key table's single filter axis.
 *
 *  There is deliberately ONE axis here, not two. A key's origin is written
 *  only alongside an override, so `origin != null` holds exactly when the
 *  state is ok / stale / orphan (`NamespaceStore::state_of` returns those
 *  three inside `if let Some(entry)` and can reach from_mod / missing only
 *  after that block). "Untranslated" and "AI" therefore cannot intersect at
 *  all, and offering them as two crossing axes offered a combination that is
 *  empty by construction rather than by data.
 *
 *  So each member is a VIEW: picking 'machine' means "show everything the AI
 *  wrote", not "narrow the current state selection down to the AI ones". Same
 *  model as `ViewFilter` in mods/installed/installed-filters.svelte.ts, where
 *  status views and state views likewise share one mutually-exclusive axis. */
export type KeyView = 'all' | 'translated' | 'missing' | 'stale' | 'orphan' | 'manual' | 'machine';

/** What the user currently sees in game for one key: their override if they
 *  have one, else the mod's own translation, else nothing — matches the
 *  precedence Minecraft's resource-pack stack actually applies. */
export function displayValue(row: KeyRow): string {
  return row.overrideValue ?? row.modValue ?? '';
}

// States that fold into "translated" — nothing for the user to do.
const TRANSLATED_STATES: ReadonlySet<KeyState> = new Set(['from_mod', 'ok']);

// Exhaustive on purpose, with no `default`: adding a member to KeyView must be
// a compile error here rather than a view that silently matches nothing.
function matchesView(row: KeyRow, view: KeyView): boolean {
  switch (view) {
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
    case 'manual':
      return row.origin === 'manual';
    case 'machine':
      return row.origin === 'machine';
  }
}

const NO_STICKY: ReadonlySet<string> = new Set();

function matchesSearch(row: KeyRow, q: string): boolean {
  if (!q) return true;
  return row.key.toLowerCase().includes(q) || row.sourceEn.toLowerCase().includes(q);
}

/** Apply the view, then the search term. Search matches the key or the English
 *  source — the two things a user knows going in — never the translated value
 *  itself: that may be in a script the user can't even read, which is the
 *  entire reason they're translating it.
 *
 *  `sticky` holds the keys the user has changed in this sitting. They are
 *  exempt from the VIEW — a row you just translated keeps its place instead of
 *  being yanked out from under you — but never from the SEARCH: a save touches
 *  only overrideValue / state / origin, so it cannot change whether a row
 *  matches what was typed, and exempting it from the search would make a
 *  searched-away row reappear. */
export function filterRows(
  rows: KeyRow[],
  search: string,
  view: KeyView,
  sticky: ReadonlySet<string> = NO_STICKY,
): KeyRow[] {
  const q = search.trim().toLowerCase();
  return rows.filter(
    (row) => (matchesView(row, view) || sticky.has(row.key)) && matchesSearch(row, q),
  );
}

/** How many of the rows `filterRows` is currently rendering are there ONLY
 *  because they are sticky — i.e. exactly the number that would disappear if
 *  the set were cleared. That is the number the refresh affordance reports and
 *  the test for whether it should exist at all.
 *
 *  It takes `search` for the same reason it takes `view`: a sticky row that the
 *  search excludes is not on screen either, so counting it would promise the
 *  user a row they cannot see. Both functions share `matchesSearch` so the two
 *  answers cannot drift apart. Under 'all', and for a row that re-entered its
 *  view (an override cleared back to 'missing' under the Untranslated view),
 *  this is zero and there is nothing to offer. */
export function stickyOutOfView(
  rows: KeyRow[],
  search: string,
  view: KeyView,
  sticky: ReadonlySet<string>,
): number {
  if (sticky.size === 0) return 0;
  const q = search.trim().toLowerCase();
  let out = 0;
  for (const row of rows) {
    if (sticky.has(row.key) && !matchesView(row, view) && matchesSearch(row, q)) out++;
  }
  return out;
}

export type OriginCounts = { manual: number; machine: number };

/** How many rows carry each override origin, in one pass.
 *
 *  Always call this with the FULL namespace row set, never a filtered one. The
 *  machine count does not just size a chip: it gates the bulk-revert button and
 *  is the number interpolated into that destructive confirm, while the backend
 *  revert takes no filter and drops every machine entry in the namespace. Fed a
 *  filtered subset, the dialog would under-report what it is about to delete. */
export function countOrigins(rows: KeyRow[]): OriginCounts {
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

/** Canonical chip order: the five state views, then the two origin views. */
const VIEW_ORDER: readonly KeyView[] = [
  'all',
  'translated',
  'missing',
  'stale',
  'orphan',
  'manual',
  'machine',
];

/** Views that always render: they define the surface, so hiding one at zero
 *  would make the toolbar's shape depend on the data. Everything else appears
 *  only when it has something to show — the same rule InstalledToolbar applies
 *  to Updates / Issues / Incompatible. */
const ANCHOR_VIEWS: ReadonlySet<KeyView> = new Set(['all', 'translated', 'missing']);

/** The number one chip shows. State counts and origin counts stay in separate
 *  records on purpose: the four state buckets partition `all`, whereas manual
 *  and machine OVERLAP translated (a machine-written 'ok' row is in both) and
 *  a from_mod row is in neither. Merging them into FilterCounts would break
 *  that partition. */
export function viewCount(view: KeyView, counts: FilterCounts, origins: OriginCounts): number {
  switch (view) {
    case 'all':
      return counts.all;
    case 'translated':
      return counts.translated;
    case 'missing':
      return counts.missing;
    case 'stale':
      return counts.stale;
    case 'orphan':
      return counts.orphan;
    case 'manual':
      return origins.manual;
    case 'machine':
      return origins.machine;
  }
}

/** Which views render, in canonical order.
 *
 *  `keepView` is the active view when it currently holds sticky rows. Without
 *  it, saving the last "needs review" key would drop that count to zero, the
 *  chip would disappear, and KeyTable's fallback effect would force the view
 *  back to 'all' — flooding the table with the whole mod on exactly the action
 *  stickiness exists to make undisruptive. It also makes the count-gated views
 *  behave like the anchors, which otherwise stay put at zero: one user action,
 *  one outcome. */
export function visibleViews(
  counts: FilterCounts,
  origins: OriginCounts,
  keepView: KeyView | null = null,
): KeyView[] {
  return VIEW_ORDER.filter(
    (view) => ANCHOR_VIEWS.has(view) || viewCount(view, counts, origins) > 0 || view === keepView,
  );
}
