/**
 * The Installed tab's graph views and search (spec 2026-09-28 §6.8, D13), wired through the
 * view: «Нужны другим» = a mod at least one other installed mod requires; «Неиспользуемые
 * библиотеки» = a platform library (`summary.library === true` — `null` means the source cannot
 * tell, never "a library"), enabled, that nothing requires. Search matches the file name and the
 * slug as well as the name. And (plan amendment A17) while a self-completing pack still has files
 * to download, its pre-flight violations are advisory: the «Проблемы» chip stays quiet, like the
 * Play gate.
 */
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => {
  const mod = (sha1: string, projectId: string, name: string, enabled = true) => ({
    filename: `${name.toLowerCase()}-1.0.jar`,
    sha1,
    source: 'modrinth',
    project_id: projectId,
    version_id: 'v',
    name,
    version_number: '1.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled,
    enrich_attempted: false,
    requires: [],
  });
  const summary = (projectId: string, name: string, library: boolean | null) => ({
    source: 'modrinth',
    project_id: projectId,
    slug: `${name.toLowerCase()}-slug`,
    name,
    summary: '',
    icon_url: null,
    downloads: 0,
    author: 'x',
    updated_at: null,
    library,
  });
  const summaries = [
    summary('PA', 'Alpha', false),
    summary('PL', 'Lib', true), // required by Alpha
    summary('PU', 'Unused', true), // required by nothing
    summary('PD', 'Dormant', true), // required by nothing, but disabled
    summary('PN', 'Nolib', null), // the source cannot tell
  ];
  return {
    rows: [
      mod('a', 'PA', 'Alpha'),
      mod('l', 'PL', 'Lib'),
      mod('u', 'PU', 'Unused'),
      mod('d', 'PD', 'Dormant', false),
      mod('n', 'PN', 'Nolib'),
    ],
    summaries,
    // Disabled mods are never graph roots; Alpha requires Lib, which is installed.
    graph: {
      roots: [
        {
          sha1: 'a',
          source: 'modrinth',
          project_id: 'PA',
          name: 'Alpha',
          required: [
            {
              source: 'modrinth',
              project_id: 'PL',
              name: 'Lib',
              installed: true,
              declared: 'required',
              cycle: false,
              children: [],
            },
          ],
          optional: [],
        },
        {
          sha1: 'l',
          source: 'modrinth',
          project_id: 'PL',
          name: 'Lib',
          required: [],
          optional: [],
        },
        {
          sha1: 'u',
          source: 'modrinth',
          project_id: 'PU',
          name: 'Unused',
          required: [],
          optional: [],
        },
        {
          sha1: 'n',
          source: 'modrinth',
          project_id: 'PN',
          name: 'Nolib',
          required: [],
          optional: [],
        },
      ],
    },
    instanceDependencyPreflight: vi.fn(),
    listInstalled: vi.fn(),
    // The view's `mod-toggle` listener, so a case can play the event a switch emits.
    onToggle: null as null | (() => void),
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: h.listInstalled,
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsProjects: vi.fn((_source: string, ids: string[]) =>
      Promise.resolve({
        status: 'ok',
        data: h.summaries.filter((s) => ids.includes(s.project_id)),
      }),
    ),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: h.graph }),
    instanceDependencyPreflight: h.instanceDependencyPreflight,
    modsResolveDepNames: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: {
      listen: (cb: () => void) => {
        h.onToggle = cb;
        return Promise.resolve(() => {});
      },
    },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import { commands } from '$lib/ipc/bindings';
import InstalledModsView from '$lib/mods/installed/InstalledModsView.svelte';

// A DISTINCT instance id per case: the pre-flight and graph caches are per-instance LRUs.
const props = (instanceId: string) => ({
  instanceId,
  mcVersion: '1.21.1',
  loader: 'neoforge' as const,
});
const shown = () =>
  [...document.querySelectorAll('[data-mod-row]')].map((el) => el.getAttribute('data-mod-row'));
const clean = { status: 'ok', data: { violations: [] } };
const rowsWith = (sha1: string, enabled: boolean) =>
  h.rows.map((r) => (r.sha1 === sha1 ? { ...r, enabled } : r));
// The graph when the platform could not describe `sha1`'s installed version (offline, say).
const depsUnknownOf = (sha1: string) => ({
  roots: h.graph.roots.map((r) =>
    r.sha1 === sha1 ? { ...r, deps_unknown: 'unreachable' as const } : r,
  ),
});

beforeEach(() => {
  h.listInstalled.mockReset();
  h.listInstalled.mockResolvedValue({ status: 'ok', data: h.rows });
});

describe('Installed — library chips and search', () => {
  it('offers «Needed by others» and «Unused libraries», each naming exactly its mods', async () => {
    h.instanceDependencyPreflight.mockResolvedValue(clean);
    render(InstalledModsView, { props: props('lib-chips') });

    const needed = await waitFor(() => screen.getByRole('radio', { name: /Needed by others/ }));
    const unused = screen.getByRole('radio', { name: /Unused libraries/ });
    expect(needed.textContent).toContain('1');
    expect(unused.textContent).toContain('1');

    await fireEvent.click(unused);
    // Not Lib (Alpha needs it), not Dormant (disabled), not Nolib (the source cannot tell).
    await waitFor(() => expect(shown()).toEqual(['modrinth:PU']));

    await fireEvent.click(screen.getByRole('radio', { name: /Needed by others/ }));
    await waitFor(() => expect(shown()).toEqual(['modrinth:PL']));
  });

  // The backend roots the graph at the ENABLED mods only, and a toggle re-reads the list before
  // the re-resolved graph lands. Until it does, the graph is stale; the chips must not repeat
  // what it says then.
  it('a mod switched off since the graph was built no longer makes its library needed', async () => {
    h.instanceDependencyPreflight.mockResolvedValue(clean);
    // Alpha is off now; the graph still roots it (built while it was on).
    h.listInstalled.mockResolvedValue({ status: 'ok', data: rowsWith('a', false) });
    render(InstalledModsView, { props: props('switched-off') });

    const unused = await waitFor(() => screen.getByRole('radio', { name: /Unused libraries/ }));
    expect(unused.textContent).toContain('2'); // Lib and Unused
    expect(screen.queryByRole('radio', { name: /Needed by others/ })).toBeNull();
  });

  it('a mod switched on since the graph was built hides the graph views until the graph knows it', async () => {
    h.instanceDependencyPreflight.mockResolvedValue(clean);
    // Dormant is on now, but the graph (built while it was off) has no root for it: what it
    // requires is unknown, and «Unused» may be exactly that.
    h.listInstalled.mockResolvedValue({ status: 'ok', data: rowsWith('d', true) });
    render(InstalledModsView, { props: props('switched-on') });

    // The graph has loaded (Alpha's row shows its relation pill) …
    await waitFor(() => {
      expect(shown()).toHaveLength(5);
      expect(document.querySelector('[data-testid="relation-pill"]')).not.toBeNull();
    });
    // … yet it does not know every enabled mod, so neither view is a fact.
    expect(screen.queryByRole('radio', { name: /Unused libraries/ })).toBeNull();
    expect(screen.queryByRole('radio', { name: /Needed by others/ })).toBeNull();
  });

  // …and the switch itself re-resolves the graph: the mod switched on gets its root, and the
  // views are facts again without a reload.
  it('a toggle re-resolves the graph, so the views come back for a mod switched on', async () => {
    h.instanceDependencyPreflight.mockResolvedValue(clean);
    render(InstalledModsView, { props: props('toggle-reresolves') });
    const unused = await waitFor(() => screen.getByRole('radio', { name: /Unused libraries/ }));
    expect(unused.textContent).toContain('1');

    // Dormant is switched on: the list re-reads it enabled, and the graph, asked again, roots it.
    const dormantRoot = {
      sha1: 'd',
      source: 'modrinth',
      project_id: 'PD',
      name: 'Dormant',
      required: [],
      optional: [],
    };
    h.listInstalled.mockResolvedValue({ status: 'ok', data: rowsWith('d', true) });
    vi.mocked(commands.modsDependencyGraph).mockResolvedValue({
      status: 'ok',
      data: { roots: [...h.graph.roots, dormantRoot] },
    } as never);
    try {
      h.onToggle?.();
      // Unused and now Dormant — a library, on, and nothing requires it.
      await waitFor(
        () =>
          expect(screen.getByRole('radio', { name: /Unused libraries/ }).textContent).toContain(
            '2',
          ),
        { timeout: 3000 },
      );
    } finally {
      vi.mocked(commands.modsDependencyGraph).mockResolvedValue({
        status: 'ok',
        data: h.graph,
      } as never);
    }
  });

  // Offline (or an unidentified version) what a mod needs is unknown, and a library it needs
  // would read as unused. While an enabled mod's dependencies are unknown, «Unused libraries»
  // claims nothing; «Needed by others» stays — every edge it counts is real (a lower bound).
  it('calls no library unused while an enabled mod’s dependencies are unknown', async () => {
    h.instanceDependencyPreflight.mockResolvedValue(clean);
    vi.mocked(commands.modsDependencyGraph).mockResolvedValue({
      status: 'ok',
      data: depsUnknownOf('n'),
    } as never);
    try {
      render(InstalledModsView, { props: props('deps-unknown') });

      const needed = await waitFor(() => screen.getByRole('radio', { name: /Needed by others/ }));
      expect(needed.textContent).toContain('1');
      expect(screen.queryByRole('radio', { name: /Unused libraries/ })).toBeNull();
      // The mod itself says so where its dependency count would be.
      expect(screen.getByRole('button', { name: /dependencies unknown/ })).toBeTruthy();

      // The platform answers again: re-checked, the graph knows every mod and the view is a fact.
      vi.mocked(commands.modsDependencyGraph).mockResolvedValue({
        status: 'ok',
        data: h.graph,
      } as never);
      await fireEvent.click(screen.getByRole('button', { name: /Re-check deps/ }));
      await waitFor(
        () =>
          expect(screen.getByRole('radio', { name: /Unused libraries/ }).textContent).toContain(
            '1',
          ),
        { timeout: 3000 },
      );
    } finally {
      vi.mocked(commands.modsDependencyGraph).mockResolvedValue({
        status: 'ok',
        data: h.graph,
      } as never);
    }
  });

  // Like «Needed by others» above: a root switched off since the graph was built requires nothing
  // at load time, so what it might require no longer keeps a library from reading as unused.
  it('a mod switched off since no longer holds back «Unused libraries», however unknown its dependencies', async () => {
    h.instanceDependencyPreflight.mockResolvedValue(clean);
    h.listInstalled.mockResolvedValue({ status: 'ok', data: rowsWith('n', false) });
    vi.mocked(commands.modsDependencyGraph).mockResolvedValue({
      status: 'ok',
      data: depsUnknownOf('n'),
    } as never);
    try {
      render(InstalledModsView, { props: props('deps-unknown-switched-off') });
      const unused = await waitFor(() => screen.getByRole('radio', { name: /Unused libraries/ }));
      expect(unused.textContent).toContain('1');
    } finally {
      vi.mocked(commands.modsDependencyGraph).mockResolvedValue({
        status: 'ok',
        data: h.graph,
      } as never);
    }
  });

  it('finds a mod by its file name or its slug, not only its name', async () => {
    h.instanceDependencyPreflight.mockResolvedValue(clean);
    render(InstalledModsView, { props: props('lib-search') });
    await waitFor(() => expect(shown()).toHaveLength(5));
    const search = screen.getByRole('searchbox', { name: 'Filter installed mods' });

    await fireEvent.input(search, { target: { value: 'DORMANT-1.0.jar' } });
    await waitFor(() => expect(shown()).toEqual(['modrinth:PD']));

    await fireEvent.input(search, { target: { value: 'unused-slug' } });
    await waitFor(() => expect(shown()).toEqual(['modrinth:PU']));
  });

  it('keeps the Issues chip, the panel and the row line quiet while a self-completing pack still has files to download', async () => {
    h.instanceDependencyPreflight.mockResolvedValue({
      status: 'ok',
      data: {
        violations: [
          {
            dependent_sha1: 'a',
            dependent_name: 'Alpha',
            dep_id: 'lib2',
            kind: 'missing_required',
            installed_version: null,
            needed: '',
            needed_desc: {
              raw: '',
              family: 'maven',
              alternatives: [],
              unparseable: false,
              soft: false,
            },
            provider_project: null,
            provider_sha1: null,
            family: null,
          },
        ],
        pack_completion: {
          total: 2,
          outstanding: [
            { display_name: 'Lib Two', pattern: 'lib2-*.jar', url: null, destination: 'mods' },
          ],
        },
      },
    });
    render(InstalledModsView, { props: props('self-completing') });
    // The rows are in (so the chip group renders) and the pre-flight has answered: the view asks
    // for the names its report needs. (The panel is no signal — it stays quiet too, below.) …
    await waitFor(() => {
      expect(shown()).toHaveLength(5);
      expect(commands.modsResolveDepNames).toHaveBeenCalledWith('self-completing', [
        { dependent_sha1: 'a', dep_id: 'lib2' },
      ]);
    });
    expect(screen.getByRole('radio', { name: /All/ })).toBeTruthy();
    // … yet nothing blocks: the pack fills itself in on first launch. Nothing here says the game
    // won't start — not the chip, not «What stops the game», not the row (the gate's predicate).
    expect(screen.queryByRole('radio', { name: /Issues/ })).toBeNull();
    expect(document.querySelector('[data-testid="preflight-panel"]')).toBeNull();
    expect(document.querySelector('[data-testid="row-problem"]')).toBeNull();
  });
});
