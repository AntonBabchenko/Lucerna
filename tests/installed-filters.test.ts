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

describe('createInstalledFilters', () => {
  it('counts total/enabled/disabled over the unfiltered rows', () => {
    const rows = [row('1', 'A', true), row('2', 'B', false), row('3', 'C', true)];
    const f = createInstalledFilters(
      () => rows,
      () => new Set(),
      () => new Set(),
    );
    expect(f.counts.total).toBe(3);
    expect(f.counts.enabled).toBe(2);
    expect(f.counts.disabled).toBe(1);
  });

  it('filters by name (case-insensitive)', () => {
    const rows = [row('1', 'Sodium', true), row('2', 'Lithium', true)];
    const f = createInstalledFilters(
      () => rows,
      () => new Set(),
      () => new Set(),
    );
    f.filter = 'sod';
    expect(f.filtered.map((r) => r.installed.name)).toEqual(['Sodium']);
  });

  it('enabledFilter narrows to enabled / disabled', () => {
    const rows = [row('1', 'A', true), row('2', 'B', false)];
    const f = createInstalledFilters(
      () => rows,
      () => new Set(),
      () => new Set(),
    );
    f.enabledFilter = 'disabled';
    expect(f.filtered.map((r) => r.installed.name)).toEqual(['B']);
  });

  it('quickFilter "updates" keeps only updatable shas', () => {
    const rows = [row('1', 'A', true), row('2', 'B', true)];
    const f = createInstalledFilters(
      () => rows,
      () => new Set(['2']),
      () => new Set(),
    );
    f.quickFilter = 'updates';
    expect(f.filtered.map((r) => r.installed.sha1)).toEqual(['2']);
  });

  it('quickFilter "issues" keeps only shas with missing deps', () => {
    const rows = [row('1', 'A', true), row('2', 'B', true)];
    const f = createInstalledFilters(
      () => rows,
      () => new Set(),
      () => new Set(['1']),
    );
    f.quickFilter = 'issues';
    expect(f.filtered.map((r) => r.installed.sha1)).toEqual(['1']);
  });

  it('paginates and clamps the page when the list shrinks', () => {
    const rows = Array.from({ length: 5 }, (_, i) => row(String(i + 1), `M${i}`, true));
    const f = createInstalledFilters(
      () => rows,
      () => new Set(),
      () => new Set(),
    );
    f.pageSize = 2;
    f.page = 2; // last page (index 4)
    expect(f.paged).toHaveLength(1);
    expect(f.pageCount).toBe(3);
  });

  it('sorts by name desc', () => {
    const rows = [row('1', 'A', true), row('2', 'B', true)];
    const f = createInstalledFilters(
      () => rows,
      () => new Set(),
      () => new Set(),
    );
    f.sortBy = 'name-desc';
    expect(f.filtered.map((r) => r.installed.name)).toEqual(['B', 'A']);
  });
});
