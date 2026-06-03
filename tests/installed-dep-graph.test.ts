import { describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  modsDependencyGraph: vi.fn(),
  modsVersions: vi.fn(),
  modsInstallWithDeps: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));
vi.mock('$lib/toasts/toasts.svelte', () => ({ pushSuccess: vi.fn(), pushWarning: vi.fn() }));
vi.mock('$lib/i18n', () => ({ t: { subscribe: () => () => {} } }));
vi.mock('svelte/store', () => ({ get: () => (k: string) => k }));
vi.mock('$lib/mods/dep-graph-cache', () => ({ depGraphCache: new Map() }));

// The seed-from-cache $effect fires reloadGraphNow() on construction (this
// vitest config compiles runes, so the effect runs rather than being inert).
// Give the graph command a benign default so that auto-fire never rejects in
// tests that don't configure it; each test overrides it as needed.
mocks.modsDependencyGraph.mockResolvedValue({ status: 'ok', data: { roots: [] } });

import { createDepGraph } from '$lib/mods/installed/dep-graph.svelte';
import type { Row } from '$lib/mods/installed/installed-data.svelte';

const row = (sha1: string, name: string): Row => ({
  summary: null,
  installed: {
    filename: `${sha1}.jar`,
    sha1,
    source: 'modrinth',
    project_id: 'P' + sha1,
    version_id: 'v',
    name,
    version_number: '1.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled: true,
    enrich_attempted: false,
  },
});

const ctx = {
  getMcVersion: () => '1.20.1',
  getLoader: () => 'fabric' as const,
  refresh: async () => {},
  getFiltered: () => [] as Row[],
  setPage: () => {},
  getPageSize: () => 50,
};

describe('createDepGraph', () => {
  it('depCounts walks the required subtree and counts missing', () => {
    const d = createDepGraph(
      () => 'i',
      () => [],
      ctx,
    );
    // depCounts only reads `.required`; the literal omits the unused
    // source/project_id root fields, so type it as the param shape.
    const root: Parameters<typeof d.depCounts>[0] = {
      sha1: 'a',
      source: 'modrinth',
      project_id: 'Pa',
      name: 'A',
      required: [
        {
          source: 'modrinth',
          project_id: 'X',
          name: 'X',
          status: 'satisfied',
          cycle: false,
          children: [
            {
              source: 'modrinth',
              project_id: 'Y',
              name: 'Y',
              status: 'missing_required',
              cycle: false,
              children: [],
            },
          ],
        },
      ],
      optional: [],
    };
    expect(d.depCounts(root)).toEqual({ total: 2, missing: 1 });
  });

  it('missingShas reports roots with >=1 missing required dep', async () => {
    mocks.modsDependencyGraph.mockResolvedValue({
      status: 'ok',
      data: {
        roots: [
          {
            sha1: 'a',
            name: 'A',
            required: [
              {
                source: 'modrinth',
                project_id: 'Y',
                name: 'Y',
                status: 'missing_required',
                cycle: false,
                children: [],
              },
            ],
            optional: [],
          },
          { sha1: 'b', name: 'B', required: [], optional: [] },
        ],
      },
    });
    const d = createDepGraph(
      () => 'i',
      () => [row('a', 'A'), row('b', 'B')],
      ctx,
    );
    await d.reloadGraphNow();
    expect([...d.missingShas]).toEqual(['a']);
  });

  it('toggleExpand reassigns the expanded set immutably', () => {
    const d = createDepGraph(
      () => 'i',
      () => [],
      ctx,
    );
    const before = d.expanded;
    d.toggleExpand('a');
    expect(d.expanded).not.toBe(before);
    expect(d.expanded.has('a')).toBe(true);
  });

  it('requiredBy maps project_id to the resolved root display names', async () => {
    mocks.modsDependencyGraph.mockResolvedValue({
      status: 'ok',
      data: {
        roots: [
          {
            sha1: 'a',
            name: 'release-1.2.3',
            required: [
              {
                source: 'modrinth',
                project_id: 'Pb',
                name: 'B',
                status: 'satisfied',
                cycle: false,
                children: [],
              },
            ],
            optional: [],
          },
        ],
      },
    });
    // Row 'a' has resolved summary name "Alpha"; the graph root name is the
    // release title "release-1.2.3" — requiredBy must prefer the resolved name.
    const rows = [
      {
        ...row('a', 'release-1.2.3'),
        summary: {
          source: 'modrinth',
          project_id: 'Pa',
          slug: 's',
          name: 'Alpha',
          summary: '',
          icon_url: null,
          downloads: 1,
          author: 'x',
          updated_at: null,
        },
      } as Row,
      row('b', 'B'),
    ];
    const d = createDepGraph(
      () => 'i',
      () => rows,
      ctx,
    );
    await d.reloadGraphNow();
    expect(d.requiredBy.get('Pb')).toEqual(['Alpha']);
  });
});
