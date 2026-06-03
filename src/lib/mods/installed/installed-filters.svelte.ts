import type { Row } from './installed-data.svelte';
import { rowDisplayName } from './row-utils';

export type EnabledFilter = 'all' | 'enabled' | 'disabled';
export type QuickFilter = 'all' | 'updates' | 'issues';
export type SortBy = 'name-asc' | 'name-desc' | 'recent' | 'source';

// Owns the filter / sort / pagination math for the installed list. `filtered`
// is the whole matching set (selection + dep graph span this); `paged` is just
// the rendered slice. `getUpdatableShas` / `getMissingShas` feed the
// updates / issues quick-filters and the toolbar counts.
export function createInstalledFilters(
  getRows: () => Row[],
  getUpdatableShas: () => Set<string>,
  getMissingShas: () => Set<string>,
) {
  let filter = $state('');
  let enabledFilter = $state<EnabledFilter>('all');
  let quickFilter = $state<QuickFilter>('all');
  let sortBy = $state<SortBy>('name-asc');
  let pageSize = $state<number>(50);
  let page = $state(0);

  const sorted = $derived.by(() => {
    const xs = [...getRows()];
    const nameLower = (r: Row) => rowDisplayName(r).toLowerCase();
    switch (sortBy) {
      case 'name-asc':
        return xs.sort((a, b) => nameLower(a).localeCompare(nameLower(b)));
      case 'name-desc':
        return xs.sort((a, b) => nameLower(b).localeCompare(nameLower(a)));
      case 'recent':
        return xs.sort((a, b) => b.installed.installed_at.localeCompare(a.installed.installed_at));
      case 'source':
        return xs.sort((a, b) => {
          const sa = a.installed.source ?? 'zz-manual';
          const sb = b.installed.source ?? 'zz-manual';
          if (sa !== sb) return sa.localeCompare(sb);
          return nameLower(a).localeCompare(nameLower(b));
        });
    }
  });

  const filtered = $derived.by(() => {
    const updatable = getUpdatableShas();
    const missing = getMissingShas();
    return sorted
      .filter((r) => {
        if (enabledFilter === 'enabled') return r.installed.enabled;
        if (enabledFilter === 'disabled') return !r.installed.enabled;
        return true;
      })
      .filter((r) => {
        if (quickFilter === 'updates') return updatable.has(r.installed.sha1);
        if (quickFilter === 'issues') return missing.has(r.installed.sha1);
        return true;
      })
      .filter(
        (r) =>
          filter.trim() === '' || rowDisplayName(r).toLowerCase().includes(filter.toLowerCase()),
      );
  });

  const pageCount = $derived(Math.max(1, Math.ceil(filtered.length / pageSize)));
  const paged = $derived(filtered.slice(page * pageSize, page * pageSize + pageSize));

  const counts = $derived.by(() => {
    const rows = getRows();
    const enabled = rows.filter((r) => r.installed.enabled).length;
    return {
      total: rows.length,
      enabled,
      disabled: rows.length - enabled,
      updates: getUpdatableShas().size,
      issues: getMissingShas().size,
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
        void enabledFilter;
        void quickFilter;
        void sortBy;
        void pageSize;
        page = 0;
      });
      $effect(() => {
        if (page > pageCount - 1) page = Math.max(0, pageCount - 1);
      });
      // When the active quick-filter's set empties out (user fixed the last
      // dependency problem or applied the last update), its chip + the attention
      // bar disappear — so auto-reset to 'all' instead of stranding an empty list
      // with no control to clear the filter.
      $effect(() => {
        if (quickFilter === 'updates' && getUpdatableShas().size === 0) quickFilter = 'all';
        else if (quickFilter === 'issues' && getMissingShas().size === 0) quickFilter = 'all';
      });
    });
  } catch {
    /* no Svelte runtime (vitest) — effects inert, which is what unit tests want */
  }

  return {
    get filter() {
      return filter;
    },
    set filter(v: string) {
      filter = v;
    },
    get enabledFilter() {
      return enabledFilter;
    },
    set enabledFilter(v: EnabledFilter) {
      enabledFilter = v;
    },
    get quickFilter() {
      return quickFilter;
    },
    set quickFilter(v: QuickFilter) {
      quickFilter = v;
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
