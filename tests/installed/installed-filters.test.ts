import { flushSync } from 'svelte';
import { SvelteSet } from 'svelte/reactivity';
import { describe, expect, it } from 'vitest';
import {
  createInstalledFilters,
  type StatusPredicates,
} from '$lib/mods/installed/installed-filters.svelte';

type R = { id: string; name: string; filename: string; slug: string | null };
const rows: R[] = [
  { id: 'a', name: 'Alpha', filename: 'alpha-fabric-1.2.jar', slug: 'alpha' },
  { id: 'b', name: 'Bravo', filename: 'bravo.jar', slug: 'bravo-lib' },
  { id: 'c', name: 'Charlie', filename: 'charlie.jar', slug: null },
];
const toFilterRow = (r: R) => ({
  id: r.id,
  name: r.name,
  enabled: true,
  sortKey: '',
  source: 'modrinth',
  searchTerms: [r.filename, r.slug],
});
const withStatus = (status: StatusPredicates) =>
  createInstalledFilters(() => rows, toFilterRow, status);
const make = (graphLoaded: boolean, needed = new Set(['b']), unused = new Set(['c'])) =>
  withStatus({
    isNeeded: (id) => needed.has(id),
    isUnusedLibrary: (id) => unused.has(id),
    graphReady: () => graphLoaded,
  });

describe('installed-filters — graph views and search', () => {
  it('"needed" keeps the mods other mods require', () => {
    const f = make(true);
    f.viewFilter = 'needed';
    expect(f.filtered.map((r) => r.id)).toEqual(['b']);
    expect(f.counts.needed).toBe(1);
    f.dispose();
  });

  it('"unusedLibraries" keeps libraries nothing requires', () => {
    const f = make(true);
    f.viewFilter = 'unusedLibraries';
    expect(f.filtered.map((r) => r.id)).toEqual(['c']);
    expect(f.counts.unusedLibraries).toBe(1);
    f.dispose();
  });

  it('counts nothing before the graph has loaded — a count before load is not a fact', () => {
    const f = make(false);
    expect(f.counts.needed).toBe(0);
    expect(f.counts.unusedLibraries).toBe(0);
    f.dispose();
  });

  it('a caller without the graph predicates never counts the graph views', () => {
    const f = withStatus({});
    expect(f.counts.needed).toBe(0);
    expect(f.counts.unusedLibraries).toBe(0);
    f.dispose();
  });

  // The auto-reset runs in the composable's own effects, which are live under vitest; flushSync
  // runs them now. A graph view with nothing in it strands the user on an empty list, but a 0
  // counted before the graph has loaded is not "nothing" — the view must survive that.
  it('falls back to "all" when a graph view is empty — but only once the graph has loaded', () => {
    const loaded = new SvelteSet<string>();
    const f = withStatus({
      isNeeded: () => false,
      isUnusedLibrary: () => false,
      graphReady: () => loaded.has('graph'),
    });
    f.viewFilter = 'needed';
    flushSync();
    expect(f.viewFilter).toBe('needed');
    loaded.add('graph');
    flushSync();
    expect(f.viewFilter).toBe('all');
    f.dispose();
  });

  it('search matches the filename and the slug as well as the name, case-insensitively', () => {
    const f = make(true);
    f.filter = 'FABRIC-1.2';
    expect(f.filtered.map((r) => r.id)).toEqual(['a']);
    f.filter = 'bravo-LIB';
    expect(f.filtered.map((r) => r.id)).toEqual(['b']);
    f.filter = 'charl';
    expect(f.filtered.map((r) => r.id)).toEqual(['c']);
    f.dispose();
  });
});
