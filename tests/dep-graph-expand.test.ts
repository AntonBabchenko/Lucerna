import { tick } from 'svelte';
import { describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({ commands: {}, events: {} }));

import { createDepGraph } from '../src/lib/mods/installed/dep-graph.svelte';

const ctx = {
  getMcVersion: () => null,
  getLoader: () => null,
  refresh: async () => {},
  getFiltered: () => [],
  setPage: () => {},
  getPageSize: () => 20,
};

describe('createDepGraph expand', () => {
  // The deps tour opens a mod's panel from its second step; Back and Next run it again, and a
  // panel the user already opened must stay open — `toggleExpand` would close it.
  it('opens a panel and leaves an open one open', async () => {
    const g = createDepGraph(
      () => null,
      () => [],
      ctx,
    );
    await tick(); // the instance effect's first run resets `expanded`
    g.expand('a');
    expect(g.expanded.has('a')).toBe(true);
    g.expand('a');
    expect([...g.expanded]).toEqual(['a']);
    g.toggleExpand('a');
    expect(g.expanded.has('a')).toBe(false);
    g.dispose();
  });
});
