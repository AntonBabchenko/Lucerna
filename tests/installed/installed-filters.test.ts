import { describe, expect, it } from 'vitest';
import { createInstalledFilters } from '$lib/mods/installed/installed-filters.svelte';

function row(sha1: string, name: string) {
  return {
    installed: {
      sha1,
      name,
      enabled: true,
      source: 'modrinth',
      project_id: sha1,
      version_id: 'v',
      installed_at: '2026-01-01T00:00:00Z',
      filename: `${name}.jar`,
      version_number: '1',
      enrich_attempted: false,
      requires: [],
    },
    summary: null,
  } as never;
}

describe('installed-filters incompatible view-filter', () => {
  it('shows only incompatible rows when viewFilter = incompatible', () => {
    const rows = [row('a', 'Alpha'), row('b', 'Bravo'), row('c', 'Charlie')];
    const incompatible = new Set(['b']);
    const f = createInstalledFilters(
      () => rows,
      () => new Set(),
      () => new Set(),
      () => incompatible,
    );
    f.viewFilter = 'incompatible';
    expect(f.filtered.map((r) => r.installed.sha1)).toEqual(['b']);
    expect(f.counts.incompatible).toBe(1);
    f.dispose();
  });
});
