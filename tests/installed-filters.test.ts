import { describe, expect, it } from 'vitest';
import type { ModSource } from '$lib/ipc/bindings';
import type { Row } from '$lib/mods/installed/installed-data.svelte';
import { createInstalledFilters } from '$lib/mods/installed/installed-filters.svelte';

const row = (
  sha1: string,
  name: string,
  enabled: boolean,
  source: ModSource | null = 'modrinth',
): Row => ({
  summary: null,
  installed: {
    filename: `${sha1}.jar`,
    sha1,
    source,
    project_id: source ? `p${sha1}` : null,
    version_id: 'v',
    name,
    version_number: '1.0',
    installed_at: `2026-01-0${sha1}T00:00:00Z`,
    enabled,
    enrich_attempted: false,
  },
});

// The client's projection to the abstract FilterRow the composable filters over.
const toFilterRow = (r: Row) => ({
  id: r.installed.sha1,
  name: r.summary?.name ?? r.installed.name,
  enabled: r.installed.enabled,
  sortKey: r.installed.installed_at,
  source: r.installed.source,
});

describe('createInstalledFilters', () => {
  it('counts total/enabled/disabled over the unfiltered rows', () => {
    const rows = [row('1', 'A', true), row('2', 'B', false), row('3', 'C', true)];
    const f = createInstalledFilters(() => rows, toFilterRow);
    expect(f.counts.total).toBe(3);
    expect(f.counts.enabled).toBe(2);
    expect(f.counts.disabled).toBe(1);
  });

  it('filters by name (case-insensitive)', () => {
    const rows = [row('1', 'Sodium', true), row('2', 'Lithium', true)];
    const f = createInstalledFilters(() => rows, toFilterRow);
    f.filter = 'sod';
    expect(f.filtered.map((r) => r.installed.name)).toEqual(['Sodium']);
  });

  it('viewFilter narrows to enabled / disabled', () => {
    const rows = [row('1', 'A', true), row('2', 'B', false)];
    const f = createInstalledFilters(() => rows, toFilterRow);
    f.viewFilter = 'disabled';
    expect(f.filtered.map((r) => r.installed.name)).toEqual(['B']);
  });

  it('viewFilter "updates" keeps only updatable shas', () => {
    const rows = [row('1', 'A', true), row('2', 'B', true)];
    const f = createInstalledFilters(() => rows, toFilterRow, {
      isUpdatable: (id) => id === '2',
    });
    f.viewFilter = 'updates';
    expect(f.filtered.map((r) => r.installed.sha1)).toEqual(['2']);
  });

  it('viewFilter "issues" keeps only shas with missing deps', () => {
    const rows = [row('1', 'A', true), row('2', 'B', true)];
    const f = createInstalledFilters(() => rows, toFilterRow, {
      hasIssue: (id) => id === '1',
    });
    f.viewFilter = 'issues';
    expect(f.filtered.map((r) => r.installed.sha1)).toEqual(['1']);
  });

  it('view filters are mutually exclusive — selecting updates does not also keep the enabled state', () => {
    // A disabled mod that has an update: 'updates' shows it regardless of the
    // enabled state (no AND-combination with enabled/disabled anymore).
    const rows = [row('1', 'A', true), row('2', 'B', false)];
    const f = createInstalledFilters(() => rows, toFilterRow, {
      isUpdatable: (id) => id === '2',
    });
    f.viewFilter = 'updates';
    expect(f.filtered.map((r) => r.installed.sha1)).toEqual(['2']);
  });

  it('paginates and clamps the page when the list shrinks', () => {
    const rows = Array.from({ length: 5 }, (_, i) => row(String(i + 1), `M${i}`, true));
    const f = createInstalledFilters(() => rows, toFilterRow);
    f.pageSize = 2;
    f.page = 2; // last page (index 4)
    expect(f.paged).toHaveLength(1);
    expect(f.pageCount).toBe(3);
  });

  it('sorts by name desc', () => {
    const rows = [row('1', 'A', true), row('2', 'B', true)];
    const f = createInstalledFilters(() => rows, toFilterRow);
    f.sortBy = 'name-desc';
    expect(f.filtered.map((r) => r.installed.name)).toEqual(['B', 'A']);
  });
});
