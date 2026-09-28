import { untrack } from 'svelte';

// A single mutually-exclusive view filter — exactly one is active at a time, so
// picking any chip simply shows that subset (no AND-combination to reason about).
// 'updates' / 'issues' are status views ('issues' = the one problem model:
// blocking ∪ warning; «Несовместимые» was folded into it, spec 2026-09-28 §6.2);
// 'needed' / 'unusedLibraries' are dependency-graph views; 'enabled' /
// 'disabled' are state views.
export type ViewFilter =
  | 'all'
  | 'enabled'
  | 'disabled'
  | 'updates'
  | 'issues'
  | 'needed'
  | 'unusedLibraries';
export type SortBy = 'name-asc' | 'name-desc' | 'recent' | 'source';

// The abstract projection every caller's row is reduced to. Keeping the filter /
// sort / count math over this domain-agnostic shape lets the client (mods) and
// the server panes (mods / plugins) share one composable while their `filtered`
// / `paged` still yield the caller's own row type `R` untouched.
export type FilterRow = {
  id: string;
  name: string;
  enabled: boolean;
  // Opaque sort key for the 'recent' order (newest first). Callers with no
  // timestamp pass '' so 'recent' collapses to the stable input order.
  sortKey: string;
  source: string | null;
  // Extra strings the search box matches besides `name` (the client: file name
  // and slug). A null entry is skipped.
  searchTerms?: readonly (string | null)[];
};

// Optional status predicates keyed by the row's `id`. The client injects all of
// them; server panes pass none (or only `isUpdatable`), so the corresponding
// chips stay at 0 and can't activate.
export type StatusPredicates = {
  isUpdatable?: (id: string) => boolean;
  hasIssue?: (id: string) => boolean;
  /** Required by at least one other installed mod (dependency graph). */
  isNeeded?: (id: string) => boolean;
  /** A platform "library", enabled, required by nothing (dependency graph). */
  isUnusedLibrary?: (id: string) => boolean;
  /** The graph behind the two views above has loaded; before that their counts are not facts. */
  graphReady?: () => boolean;
};

// Owns the filter / sort / pagination math for an installed list. `filtered` is
// the whole matching set (selection + dep graph span this); `paged` is just the
// rendered slice. `toFilterRow` projects each caller row `R` to the abstract
// FilterRow; `status` feeds the status / graph quick-filters and the toolbar
// counts.
//
// `isReady` tells the auto-reset below whether a count of 0 is a fact or merely
// "not loaded yet". Callers that can render before their rows arrive MUST pass
// it; the default is the safe one for callers whose rows are always present.
export function createInstalledFilters<R>(
  getRows: () => R[],
  toFilterRow: (row: R) => FilterRow,
  status: StatusPredicates = {},
  isReady: () => boolean = () => true,
) {
  let filter = $state('');
  let viewFilter = $state<ViewFilter>('all');
  let sortBy = $state<SortBy>('name-asc');
  let pageSize = $state<number>(50);
  let page = $state(0);

  const isUpdatable = (id: string) => status.isUpdatable?.(id) ?? false;
  const hasIssue = (id: string) => status.hasIssue?.(id) ?? false;
  const isNeeded = (id: string) => status.isNeeded?.(id) ?? false;
  const isUnusedLibrary = (id: string) => status.isUnusedLibrary?.(id) ?? false;
  const graphReady = () => status.graphReady?.() ?? true;

  const projected = $derived.by(() => getRows().map((row) => ({ row, fr: toFilterRow(row) })));

  const sorted = $derived.by(() => {
    const xs = [...projected];
    const nameLower = (p: { fr: FilterRow }) => p.fr.name.toLowerCase();
    switch (sortBy) {
      case 'name-asc':
        return xs.sort((a, b) => nameLower(a).localeCompare(nameLower(b)));
      case 'name-desc':
        return xs.sort((a, b) => nameLower(b).localeCompare(nameLower(a)));
      case 'recent':
        return xs.sort((a, b) => b.fr.sortKey.localeCompare(a.fr.sortKey));
      case 'source':
        return xs.sort((a, b) => {
          const sa = a.fr.source ?? 'zz-manual';
          const sb = b.fr.source ?? 'zz-manual';
          if (sa !== sb) return sa.localeCompare(sb);
          return nameLower(a).localeCompare(nameLower(b));
        });
    }
  });

  const filteredPairs = $derived.by(() =>
    sorted
      .filter(({ fr }) => {
        switch (viewFilter) {
          case 'enabled':
            return fr.enabled;
          case 'disabled':
            return !fr.enabled;
          case 'updates':
            return isUpdatable(fr.id);
          case 'issues':
            return hasIssue(fr.id);
          case 'needed':
            return isNeeded(fr.id);
          case 'unusedLibraries':
            return isUnusedLibrary(fr.id);
          default:
            return true; // 'all'
        }
      })
      // Text search is orthogonal and always applies on top of the view filter:
      // the name, plus any extra terms the caller projects (file name, slug),
      // case-insensitively.
      .filter(({ fr }) => {
        if (filter.trim() === '') return true;
        const q = filter.toLowerCase();
        return [fr.name, ...(fr.searchTerms ?? [])].some((s) => s?.toLowerCase().includes(q));
      }),
  );

  const filtered = $derived(filteredPairs.map((p) => p.row));
  const pageCount = $derived(Math.max(1, Math.ceil(filtered.length / pageSize)));
  const paged = $derived(filtered.slice(page * pageSize, page * pageSize + pageSize));

  const counts = $derived.by(() => {
    const frs = projected.map((p) => p.fr);
    const enabled = frs.filter((fr) => fr.enabled).length;
    return {
      total: frs.length,
      enabled,
      disabled: frs.length - enabled,
      // Only count a status when its predicate was injected — an absent
      // predicate means the caller has no such view, so the chip stays at 0.
      updates: status.isUpdatable ? frs.filter((fr) => isUpdatable(fr.id)).length : 0,
      issues: status.hasIssue ? frs.filter((fr) => hasIssue(fr.id)).length : 0,
      // Graph views count only once the graph has loaded: a 0 before that is not a fact.
      needed: status.isNeeded && graphReady() ? frs.filter((fr) => isNeeded(fr.id)).length : 0,
      unusedLibraries:
        status.isUnusedLibrary && graphReady()
          ? frs.filter((fr) => isUnusedLibrary(fr.id)).length
          : 0,
    };
  });

  // Reset to page 0 whenever the result set's shape changes; clamp the page if
  // the list shrinks. Wrapped in $effect.root so the factory is unit-testable
  // and the root is torn down via dispose() on component unmount.
  let stopEffects: (() => void) | null = null;
  try {
    stopEffects = $effect.root(() => {
      $effect(() => {
        void filter;
        void viewFilter;
        void sortBy;
        void pageSize;
        page = 0;
      });
      $effect(() => {
        if (page > pageCount - 1) page = Math.max(0, pageCount - 1);
      });
      // When the active status or graph view empties out (user fixed the last
      // problem, applied the last update, removed the last unused library), its
      // chip disappears — so auto-reset to 'all' instead of stranding an empty
      // list with the now-gone filter still active.
      $effect(() => {
        // A count of 0 over a list that has not loaded is not a fact. Without
        // this guard the Overview's deep-link to «Проблемы» was defeated on
        // arrival: MainTabs mounts AddonsTab fresh on every switch, `counts`
        // iterates rows that are still `[]` for the whole mount flush, and
        // writing `viewFilter` re-runs this effect, which then reverts it to
        // 'all' before a single row exists. The graph views wait for the graph
        // the same way.
        if (!isReady()) return;
        const c = counts;
        const statusViewEmpty =
          (viewFilter === 'updates' && c.updates === 0) ||
          (viewFilter === 'issues' && c.issues === 0);
        const graphViewEmpty =
          graphReady() &&
          ((viewFilter === 'needed' && c.needed === 0) ||
            (viewFilter === 'unusedLibraries' && c.unusedLibraries === 0));
        // Wrap the self-referential write so the effect doesn't register
        // `viewFilter` as a dependency of its own assignment (it already depends
        // on it via the reads above; this keeps the update strictly one-shot).
        if (statusViewEmpty || graphViewEmpty) untrack(() => (viewFilter = 'all'));
      });
    });
  } catch {
    /* no reactive runtime to root the effects in — they stay inert. Under vitest the runtime IS
       there: the effects run at a test's first await (or `flushSync`), so a synchronous test
       reads `filtered` / `counts` before the page reset or the auto-reset has run. */
  }

  return {
    get filter() {
      return filter;
    },
    set filter(v: string) {
      filter = v;
    },
    get viewFilter() {
      return viewFilter;
    },
    set viewFilter(v: ViewFilter) {
      viewFilter = v;
    },
    get sortBy() {
      return sortBy;
    },
    set sortBy(v: SortBy) {
      sortBy = v;
    },
    get pageSize() {
      return pageSize;
    },
    set pageSize(v: number) {
      pageSize = v;
    },
    get page() {
      return page;
    },
    set page(v: number) {
      page = v;
    },
    get filtered() {
      return filtered;
    },
    get paged() {
      return paged;
    },
    get pageCount() {
      return pageCount;
    },
    get counts() {
      return counts;
    },
    dispose() {
      stopEffects?.();
    },
  };
}
