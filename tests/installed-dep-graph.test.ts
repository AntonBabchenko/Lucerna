import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { DepTreeNode } from '$lib/ipc/bindings';

const mocks = vi.hoisted(() => ({
  modsDependencyGraph: vi.fn(),
  modsVersions: vi.fn(),
  modsInstallWithDeps: vi.fn(),
  modsInstallDependency: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));
vi.mock('$lib/ipc/format-error', () => ({
  formatError: (e: unknown) => `formatted ${JSON.stringify(e)}`,
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushInfo: vi.fn(),
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
}));
vi.mock('$lib/i18n', () => ({ t: { subscribe: () => () => {} } }));
vi.mock('svelte/store', () => ({ get: () => (k: string) => k }));
vi.mock('$lib/mods/dep-graph-cache', () => ({ depGraphCache: new Map() }));

// The seed-from-cache $effect fires reloadGraphNow() on construction (this
// vitest config compiles runes, so the effect runs rather than being inert).
// Give the graph command a benign default so that auto-fire never rejects in
// tests that don't configure it; each test overrides it as needed.
mocks.modsDependencyGraph.mockResolvedValue({ status: 'ok', data: { roots: [] } });

import { bumpCrossIds, resetCrossIdsForTests } from '$lib/mods/cross-ids.svelte';
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
    resetCrossIdsForTests();
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

  // «Could not reach the platform» passes (offline, rate-limited); «the platform does not know
  // this version» does not. A graph holding the first is shown — honestly marked — but is not the
  // session's answer: the next open asks again, cheap now that the backend keeps the versions it
  // did get. The second stays until the mods change.
  describe('what the session keeps', () => {
    const root = (
      deps_unknown: 'unreachable' | 'unidentified' | null,
      required: DepTreeNode[] = [],
    ) => ({
      sha1: 'r',
      source: 'modrinth' as const,
      project_id: 'PR',
      name: 'R',
      required,
      optional: [],
      deps_unknown,
    });
    const mountAgain = async (id: string) => {
      const before = mocks.modsDependencyGraph.mock.calls.length;
      createDepGraph(
        () => id,
        () => [],
        ctx,
      );
      await new Promise((r) => setTimeout(r, 0)); // the seed effect reads the cache
      return mocks.modsDependencyGraph.mock.calls.length - before;
    };
    const load = async (id: string, data: unknown) => {
      mocks.modsDependencyGraph.mockResolvedValue({ status: 'ok', data });
      const d = createDepGraph(
        () => id,
        () => [],
        ctx,
      );
      await d.reloadGraphNow();
      return d;
    };

    it('keeps no graph the platform could not be reached for — the next open asks again', async () => {
      const { depGraphCache } = await import('$lib/mods/dep-graph-cache');
      const d = await load('offline', { roots: [root('unreachable')] });
      expect(d.graph?.roots[0]?.deps_unknown).toBe('unreachable'); // shown, and says so
      expect((depGraphCache as Map<string, unknown>).has('offline')).toBe(false);
      expect(await mountAgain('offline')).toBe(1);
    });

    it('nor one where only a mod deeper down could not be described', async () => {
      const { depGraphCache } = await import('$lib/mods/dep-graph-cache');
      const nested = { ...n('lib'), deps_unknown: 'unreachable' as const };
      await load('offline-nested', { roots: [root(null, [n('mid', [nested])])] });
      expect((depGraphCache as Map<string, unknown>).has('offline-nested')).toBe(false);
    });

    it('keeps one whose unknowns are the platform’s own answer', async () => {
      const { depGraphCache } = await import('$lib/mods/dep-graph-cache');
      await load('unidentified', { roots: [root('unidentified')] });
      expect((depGraphCache as Map<string, unknown>).has('unidentified')).toBe(true);
      expect(await mountAgain('unidentified')).toBe(0);
    });
  });

  // Spec 2026-10-08 aliases-everywhere D4: a pass that learned an id for this profile (the page
  // moves its generation) makes the graph stale — this view reads it again; another profile's
  // pass does not touch it, and a generation already above 0 when the view mounts is no change.
  describe('a learned cross-source id', () => {
    const settle = () => new Promise((r) => setTimeout(r, 0));
    const reads = (id: string) =>
      mocks.modsDependencyGraph.mock.calls.filter((c) => c[0] === id).length;

    it('reads this profile’s graph again, and only this profile’s', async () => {
      const d = createDepGraph(
        () => 'learn-p',
        () => [],
        ctx,
      );
      await settle();
      expect(reads('learn-p')).toBe(1);
      bumpCrossIds('learn-p');
      await settle();
      expect(reads('learn-p')).toBe(2);
      bumpCrossIds('learn-other');
      await settle();
      expect(reads('learn-p')).toBe(2);
      d.dispose();
    });

    it('a generation the profile already had when the view mounted is no change', async () => {
      bumpCrossIds('learn-old');
      bumpCrossIds('learn-old');
      const d = createDepGraph(
        () => 'learn-old',
        () => [],
        ctx,
      );
      await settle();
      expect(reads('learn-old')).toBe(1); // the seed's load only
      d.dispose();
    });

    it('an answer that lands after the view is gone is neither shown nor kept', async () => {
      const { depGraphCache } = await import('$lib/mods/dep-graph-cache');
      let answer!: (v: unknown) => void;
      mocks.modsDependencyGraph.mockReturnValueOnce(
        new Promise((r) => {
          answer = r;
        }),
      );
      const d = createDepGraph(
        () => 'gone',
        () => [],
        ctx,
      );
      await settle(); // the seed's load is in flight
      d.dispose();
      answer({ status: 'ok', data: { roots: [] } });
      await settle();
      expect((depGraphCache as Map<string, unknown>).has('gone')).toBe(false);
      expect(d.graph).toBeNull();
    });
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
    // Mount first: a jump only ever happens on a mounted list.
    await new Promise((r) => setTimeout(r, 0));
    expect(await d.jumpToSha1('c')).toBe(true);
    expect(page).toBe(1);
    page = -1;
    expect(await d.jumpToSha1('nope')).toBe(false);
    expect(page).toBe(-1);
  });

  // A mod write takes the SHARED claim: a refusal means another operation holds the profile — the
  // shared `instance_busy` copy ("…or the game is running") would be false here (plan A9).
  it('a tree install the profile refuses says it is busy — never that the game runs', async () => {
    const toasts = await import('$lib/toasts/toasts.svelte');
    mocks.modsVersions.mockResolvedValue({
      status: 'ok',
      data: [{ source: 'modrinth', project_id: 'PL', version_id: 'vl' }],
    });
    mocks.modsInstallWithDeps.mockResolvedValue({
      status: 'error',
      error: { kind: 'instance_busy' },
    });
    const d = createDepGraph(
      () => 'i',
      () => [],
      ctx,
    );
    const node = {
      source: 'modrinth',
      project_id: 'PL',
      name: 'Lib',
      installed: false,
      declared: 'required',
      cycle: false,
      children: [],
    } as unknown as DepTreeNode;
    await d.installDepNode(node, null);
    expect(toasts.pushWarning).toHaveBeenCalledWith('mods.browse.toastInstallFailed', [
      'mods.ops.busy',
    ]);
  });

  // The node's own spinner (spec 2026-10-07 D3): its key is in flight from the click until the
  // list is re-read — past the graph reload, which can say «installed» before the list has the
  // row — and a second click meanwhile adds nothing.
  it('a tree install keeps its node in flight until the list is re-read, and ignores a second click', async () => {
    mocks.modsVersions.mockClear();
    mocks.modsVersions.mockResolvedValue({
      status: 'ok',
      data: [{ source: 'modrinth', project_id: 'PL', version_id: 'vl' }],
    });
    let land: (v: unknown) => void = () => {};
    mocks.modsInstallWithDeps.mockReturnValue(
      new Promise((resolve) => {
        land = resolve;
      }),
    );
    let flightAtRefresh: boolean | null = null;
    const d = createDepGraph(
      () => 'i',
      () => [],
      {
        ...ctx,
        refresh: async () => {
          flightAtRefresh = d.isInstalling('modrinth:PL');
        },
      },
    );
    const node = {
      source: 'modrinth',
      project_id: 'PL',
      name: 'Lib',
      installed: false,
      declared: 'required',
      cycle: false,
      children: [],
    } as unknown as DepTreeNode;

    const first = d.installDepNode(node, null);
    expect(d.isInstalling('modrinth:PL')).toBe(true);
    await d.installDepNode(node, null);
    expect(mocks.modsVersions).toHaveBeenCalledTimes(1);

    land({ status: 'error', error: { kind: 'instance_busy' } });
    await first;
    expect(flightAtRefresh).toBe(true);
    expect(d.isInstalling('modrinth:PL')).toBe(false);
  });

  it('a tree install names the dependencies that came along with it', async () => {
    const toasts = await import('$lib/toasts/toasts.svelte');
    mocks.modsVersions.mockResolvedValue({
      status: 'ok',
      data: [{ source: 'modrinth', project_id: 'PL', version_id: 'vl' }],
    });
    mocks.modsInstallWithDeps.mockResolvedValue({
      status: 'ok',
      data: { primary_name: 'Lib', installed_dependencies: ['Api'], details: [] },
    });
    const d = createDepGraph(
      () => 'i',
      () => [],
      ctx,
    );
    await d.installDepNode(
      {
        source: 'modrinth',
        project_id: 'PL',
        name: 'Lib',
        installed: false,
        declared: 'required',
        cycle: false,
        children: [],
      } as unknown as DepTreeNode,
      null,
    );
    expect(toasts.pushSuccess).toHaveBeenCalledWith('mods.browse.toastInstalledMod', [
      'mods.updates.installedDeps',
    ]);
  });

  // The tree knows which mod declared a dependency (spec §5.6): installing it FOR that mod goes
  // through the dependency path, which records the edge on the dependent (so removing the mod can
  // offer its orphans) and refuses a project the profile already lists — a stale graph must never
  // drop a second jar into the pack. The backend picks the build; the tree asks for no versions.
  it('a tree install under an installed dependent goes through the dependency path, for that dependent', async () => {
    const toasts = await import('$lib/toasts/toasts.svelte');
    vi.mocked(toasts.pushSuccess).mockClear();
    mocks.modsVersions.mockClear();
    mocks.modsInstallWithDeps.mockClear();
    mocks.modsInstallDependency.mockResolvedValue({
      status: 'ok',
      data: { primary_name: 'Lib', installed_dependencies: ['Api'], details: [] },
    });
    const d = createDepGraph(
      () => 'i',
      () => [],
      ctx,
    );
    await d.installDepNode(libNode(), 'dependent-sha');
    expect(mocks.modsInstallDependency).toHaveBeenCalledWith(
      'i',
      'dependent-sha',
      'modrinth',
      'PL',
    );
    expect(mocks.modsInstallWithDeps).not.toHaveBeenCalled();
    expect(mocks.modsVersions).not.toHaveBeenCalled();
    expect(toasts.pushSuccess).toHaveBeenCalledWith('mods.browse.toastInstalledMod', [
      'mods.updates.installedDeps',
    ]);
  });

  // An install refused as «already installed» is no failure: like the panel's «Install», the tree
  // says so and reads the mods again — never a warning.
  it('a tree install of a project the profile already lists says so, warns of nothing and re-reads', async () => {
    const toasts = await import('$lib/toasts/toasts.svelte');
    vi.mocked(toasts.pushWarning).mockClear();
    const already = { kind: 'mods_already_installed', name: 'Lib' };
    mocks.modsInstallDependency.mockResolvedValue({ status: 'error', error: already });
    const refresh = vi.fn(async () => {});
    const d = createDepGraph(
      () => 'i',
      () => [],
      { ...ctx, refresh },
    );
    await d.installDepNode(libNode(), 'dependent-sha');
    expect(toasts.pushInfo).toHaveBeenCalledWith(`formatted ${JSON.stringify(already)}`);
    expect(toasts.pushWarning).not.toHaveBeenCalled();
    expect(refresh).toHaveBeenCalled();
  });

  // Under an absent parent nothing installed declared the node: there is no dependent to record
  // an edge on, so it stays the plain install of the newest build for the profile.
  it('a node with no installed dependent keeps the plain install', async () => {
    mocks.modsInstallDependency.mockClear();
    mocks.modsVersions.mockResolvedValue({
      status: 'ok',
      data: [{ source: 'modrinth', project_id: 'PL', version_id: 'vl' }],
    });
    mocks.modsInstallWithDeps.mockResolvedValue({
      status: 'ok',
      data: { primary_name: 'Lib', installed_dependencies: [], details: [] },
    });
    const d = createDepGraph(
      () => 'i',
      () => [],
      ctx,
    );
    await d.installDepNode(libNode(), null);
    expect(mocks.modsVersions).toHaveBeenCalledWith('modrinth', 'PL', '1.20.1', 'fabric');
    expect(mocks.modsInstallWithDeps).toHaveBeenCalledWith(
      'i',
      { source: 'modrinth', project_id: 'PL', version_id: 'vl' },
      [],
      false,
    );
    expect(mocks.modsInstallDependency).not.toHaveBeenCalled();
  });
});

const libNode = (): DepTreeNode =>
  ({
    source: 'modrinth',
    project_id: 'PL',
    name: 'Lib',
    installed: false,
    declared: 'required',
    cycle: false,
    children: [],
  }) as unknown as DepTreeNode;
