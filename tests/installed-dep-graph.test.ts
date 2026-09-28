import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { DepTreeNode } from '$lib/ipc/bindings';

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
    project_id: `P${sha1}`,
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
  beforeEach(async () => {
    const { depGraphCache } = await import('$lib/mods/dep-graph-cache');
    (depGraphCache as Map<string, unknown>).clear();
    mocks.modsDependencyGraph.mockClear();
  });

  it('depCounts walks the required subtree and counts relationships only', () => {
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
          installed: true,
          declared: 'required',
          cycle: false,
          children: [
            {
              source: 'modrinth',
              project_id: 'Y',
              name: 'Y',
              installed: false,
              declared: 'required',
              cycle: false,
              children: [],
            },
          ],
        },
      ],
      optional: [],
    };
    // No `missing` tally: the graph knows what the platform was told, not what
    // the loader enforces. The pre-flight owns "this is a problem".
    expect(d.depCounts(root)).toEqual({ total: 2 });
  });

  const n = (
    pid: string,
    children: DepTreeNode[] = [],
    declared: 'required' | 'optional' = 'required',
    cycle = false,
  ): DepTreeNode => ({
    source: 'modrinth',
    project_id: pid,
    name: pid,
    installed: true,
    declared,
    cycle,
    children,
  });
  const rootOf = (pid: string, required: DepTreeNode[]) => ({
    sha1: 'r',
    source: 'modrinth' as const,
    project_id: pid,
    name: pid,
    required,
    optional: [],
  });

  // Spec §6.3: a diamond visits a library twice, but it is one project the mod requires.
  it('depCounts counts distinct required projects, not visits (REI: 3, not 4)', () => {
    const d = createDepGraph(
      () => 'i',
      () => [],
      ctx,
    );
    const root = rootOf('REI', [n('arch'), n('cloth', [n('arch'), n('lib')])]);
    expect(d.depCounts(root)).toEqual({ total: 3 });
  });

  it('depCounts skips optional children and a cycle back to the mod itself', () => {
    const d = createDepGraph(
      () => 'i',
      () => [],
      ctx,
    );
    const root = rootOf('R', [
      n('x', [n('o', [n('deep')], 'optional'), n('R', [], 'required', true)]),
    ]);
    expect(d.depCounts(root)).toEqual({ total: 1 });
  });

  // `missingShas` is gone on purpose: it made the graph the source of the issue
  // count, and the graph only repeats the platform's claim. The replacement is
  // the per-row status in InstalledModsView (mod-status.ts, fed by the
  // pre-flight), pinned by tests/installed-issues-from-preflight.test.ts.
  it('exposes no verdict of its own', () => {
    const d = createDepGraph(
      () => 'i',
      () => [],
      ctx,
    );
    expect('missingShas' in d).toBe(false);
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
                installed: true,
                declared: 'required',
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
    // The resolved row name ("Alpha") wins over the graph root's release title.
    expect(d.requiredBy.get('Pb')?.[0]?.name).toBe('Alpha');
  });

  it('reloadGraphNow drops a stale result and resets graphLoading on instance switch', async () => {
    let release: (v: unknown) => void = () => {};
    const pending = new Promise((res) => (release = res));
    mocks.modsDependencyGraph.mockReset();
    mocks.modsDependencyGraph.mockImplementation((id: string) =>
      id === 'A'
        ? pending.then(() => ({
            status: 'ok',
            data: { roots: [{ sha1: 'z', name: 'Z', required: [], optional: [] }] },
          }))
        : Promise.resolve({ status: 'ok', data: { roots: [] } }),
    );
    let current = 'A';
    const d = createDepGraph(
      () => current,
      () => [],
      ctx,
    );
    const inflight = d.reloadGraphNow(); // for "A"
    current = 'B'; // switch mid-flight
    release(null); // "A" resolves — stale
    await inflight;
    expect(d.graphLoading).toBe(false); // reset despite discarding
    expect(d.graph?.roots ?? []).toEqual([]); // stale "A" graph NOT committed
  });

  // Every mod change re-resolves the graph, so loads overlap: the one started last is the truth,
  // whichever order the answers arrive in.
  it('an older graph load that lands late never replaces a newer one', async () => {
    const { depGraphCache } = await import('$lib/mods/dep-graph-cache');
    (depGraphCache as Map<string, unknown>).set('i', { roots: [] }); // nothing loads on mount
    const graphOf = (sha1: string) => ({
      roots: [
        {
          sha1,
          source: 'modrinth',
          project_id: `P${sha1}`,
          name: sha1,
          required: [],
          optional: [],
        },
      ],
    });
    let landOlder: () => void = () => {};
    mocks.modsDependencyGraph
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            landOlder = () => resolve({ status: 'ok', data: graphOf('older') });
          }),
      )
      .mockImplementationOnce(() => Promise.resolve({ status: 'ok', data: graphOf('newer') }));
    const d = createDepGraph(
      () => 'i',
      () => [],
      ctx,
    );
    await new Promise((r) => setTimeout(r, 0)); // mounted: the seed effect read the cache

    const older = d.reloadGraphNow();
    await d.reloadGraphNow(); // started later, answers first
    expect(d.graph?.roots[0]?.sha1).toBe('newer');
    expect(d.graphLoading).toBe(false);

    landOlder();
    await older;
    expect(d.graph?.roots[0]?.sha1).toBe('newer');
    expect(d.graphLoading).toBe(false);
  });

  it('reuses the session-cached graph without re-resolving on mount', async () => {
    const { depGraphCache } = await import('$lib/mods/dep-graph-cache');
    (depGraphCache as Map<string, unknown>).set('cachedInst', {
      roots: [
        { sha1: 'z', source: 'modrinth', project_id: 'PZ', name: 'Z', required: [], optional: [] },
      ],
    });
    const d = createDepGraph(
      () => 'cachedInst',
      () => [],
      ctx,
    );
    await new Promise((r) => setTimeout(r, 0)); // let the seed effect run
    expect(mocks.modsDependencyGraph).not.toHaveBeenCalled();
    expect(d.graph?.roots?.[0]?.sha1).toBe('z');
  });

  it('resolves the graph when the cache has no entry for the instance', async () => {
    mocks.modsDependencyGraph.mockResolvedValue({ status: 'ok', data: { roots: [] } });
    createDepGraph(
      () => 'freshInst',
      () => [],
      ctx,
    );
    await new Promise((r) => setTimeout(r, 0));
    expect(mocks.modsDependencyGraph).toHaveBeenCalled();
  });

  // The pre-flight panel knows a dependent by its jar, not its project (a manual jar has none).
  it('jumpToSha1 pages to a row by its jar and says whether it was in the list', async () => {
    const rows = [row('a', 'A'), row('b', 'B'), row('c', 'C')];
    let page = -1;
    const d = createDepGraph(
      () => 'i',
      () => rows,
      { ...ctx, getFiltered: () => rows, setPage: (n) => (page = n), getPageSize: () => 2 },
    );
    // Mount first: the seed effect's first run resets hoveredKey, and a jump
    // only ever happens on a mounted list.
    await new Promise((r) => setTimeout(r, 0));
    expect(await d.jumpToSha1('c')).toBe(true);
    expect(page).toBe(1);
    expect(d.hoveredKey).toBe('modrinth:Pc');
    page = -1;
    expect(await d.jumpToSha1('nope')).toBe(false);
    expect(page).toBe(-1);
  });
});
