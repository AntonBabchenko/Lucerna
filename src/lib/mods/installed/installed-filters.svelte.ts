import { untrack } from 'svelte';
import type { Row } from './installed-data.svelte';
import { rowDisplayName } from './row-utils';

// A single mutually-exclusive view filter — exactly one is active at a time, so
// picking any chip simply shows that subset (no AND-combination to reason about).
// 'updates' / 'issues' are status views; 'enabled' / 'disabled' are state views.
export type ViewFilter = 'all' | 'enabled' | 'disabled' | 'updates' | 'issues' | 'incompatible';
export type SortBy = 'name-asc' | 'name-desc' | 'recent' | 'source';

// Owns the filter / sort / pagination math for the installed list. `filtered`
// is the whole matching set (selection + dep graph span this); `paged` is just
// the rendered slice. `getUpdatableShas` / `getMissingShas` feed the
// updates / issues quick-filters and the toolbar counts.
export function createInstalledFilters(
  getRows: () => Row[],
  getUpdatableShas: () => Set<string>,
  getMissingShas: () => Set<string>,
  getIncompatibleShas: () => Set<string> = () => new Set<string>(),
) {
  let filter = $state('');
  let viewFilter = $state<ViewFilter>('all');
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
    return (
      sorted
        .filter((r) => {
          switch (viewFilter) {
            case 'enabled':
              return r.installed.enabled;
            case 'disabled':
              return !r.installed.enabled;
            case 'updates':
              return updatable.has(r.installed.sha1);
            case 'issues':
              return missing.has(r.installed.sha1);
            case 'incompatible':
              return getIncompatibleShas().has(r.installed.sha1);
            default:
              return true; // 'all'
          }
        })
        // Text search is orthogonal and always applies on top of the view filter.
        .filter(
          (r) =>
            filter.trim() === '' || rowDisplayName(r).toLowerCase().includes(filter.toLowerCase()),
        )
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
      incompatible: getIncompatibleShas().size,
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
      // When the active updates/issues view empties out (user fixed the last
      // dependency problem or applied the last update), its chip disappears — so
      // auto-reset to 'all' instead of stranding an empty list with the now-gone
      // filter still active.
      $effect(() => {
        const resetUpdates = viewFilter === 'updates' && getUpdatableShas().size === 0;
        const resetIssues = viewFilter === 'issues' && getMissingShas().size === 0;
        const resetIncompat = viewFilter === 'incompatible' && getIncompatibleShas().size === 0;
        // Wrap the self-referential write so the effect doesn't register
        // `viewFilter` as a dependency of its own assignment (it already depends
        // on it via the reads above; this keeps the update strictly one-shot).
        if (resetUpdates || resetIssues || resetIncompat) untrack(() => (viewFilter = 'all'));
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
