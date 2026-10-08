/**
 * Spec 2026-10-08: after the Installed list loads, the view asks the backend to learn each
 * installed jar's id on the other platform. A pass that learned something makes the dependency
 * graph stale — the backend says so, the page moves the profile's generation
 * (spec aliases-everywhere D4; `tests/page-graph-cache-eviction.test.ts`), and the graph is read
 * again (a mod installed from Modrinth is the CurseForge project an addon names). The pass is one
 * per profile at a time, shared by every view (`$lib/mods/cross-ids.svelte`).
 */
import { render, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { bumpCrossIds, resetCrossIdsForTests } from '$lib/mods/cross-ids.svelte';

const h = vi.hoisted(() => {
  const mod = (sha1: string, projectId: string, name: string, source: string) => ({
    filename: `${name.toLowerCase()}.jar`,
    sha1,
    source,
    project_id: projectId,
    version_id: 'v',
    name,
    version_number: '1.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled: true,
    enrich_attempted: false,
    requires: [],
  });
  return {
    rows: [
      mod('srp', 'MJX', 'Parasites', 'modrinth'),
      mod('addon', '514409', 'Combat Addon', 'curseforge'),
    ],
    learn: vi.fn(),
    graph: vi.fn((_id?: string) => Promise.resolve({ status: 'ok', data: { roots: [] } })),
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn(() => Promise.resolve({ status: 'ok', data: h.rows })),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: h.graph,
    modsLearnCrossIds: h.learn,
    instanceDependencyPreflight: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { violations: [] } }),
    modsResolveDepNames: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import InstalledModsView from '$lib/mods/installed/InstalledModsView.svelte';

// A DISTINCT instance id per case: the graph cache is per instance.
const props = (instanceId: string) => ({
  instanceId,
  mcVersion: '1.12.2',
  loader: 'forge' as const,
});
const graphReads = (id: string) => h.graph.mock.calls.filter((c) => c[0] === id).length;
const passes = (id: string) => h.learn.mock.calls.filter((c) => c[0] === id).length;

beforeEach(() => {
  resetCrossIdsForTests();
  h.learn.mockReset();
});

describe('learning the installed jars’ ids on the other platform', () => {
  it('the list load runs the pass once', async () => {
    h.learn.mockResolvedValue({ status: 'ok', data: { learned: 0 } });
    render(InstalledModsView, { props: props('cross-once') });

    await waitFor(() => expect(h.learn).toHaveBeenCalledWith('cross-once'));
    await waitFor(() => expect(graphReads('cross-once')).toBe(1));
    await new Promise((r) => setTimeout(r, 50));
    expect(passes('cross-once')).toBe(1);
    expect(graphReads('cross-once')).toBe(1);
  });

  // The backend's event, through the page, is the one signal: the command's own answer reloads
  // nothing here, or every learned id would read the graph twice.
  it('the pass’s answer alone reads nothing again', async () => {
    h.learn.mockResolvedValue({ status: 'ok', data: { learned: 1 } });
    render(InstalledModsView, { props: props('cross-answer') });
    await waitFor(() => expect(h.learn).toHaveBeenCalledWith('cross-answer'));
    await new Promise((r) => setTimeout(r, 50));
    expect(graphReads('cross-answer')).toBe(1);
  });

  it('a learned id — the page moves the profile’s generation — has the graph read again', async () => {
    h.learn.mockResolvedValue({ status: 'ok', data: { learned: 0 } });
    render(InstalledModsView, { props: props('cross-learned') });
    await waitFor(() => expect(graphReads('cross-learned')).toBe(1));

    bumpCrossIds('cross-learned');
    await waitFor(() => expect(graphReads('cross-learned')).toBe(2));
    // The re-read does not set off another pass: one per list load.
    await new Promise((r) => setTimeout(r, 50));
    expect(passes('cross-learned')).toBe(1);
  });

  it('a view mounted while a pass runs joins it instead of starting a second one', async () => {
    let answer!: (v: unknown) => void;
    h.learn.mockReturnValueOnce(
      new Promise((r) => {
        answer = r;
      }),
    );
    h.learn.mockResolvedValue({ status: 'ok', data: { learned: 0 } });
    const first = render(InstalledModsView, { props: props('cross-remount') });
    await waitFor(() => expect(passes('cross-remount')).toBe(1));
    first.unmount();
    render(InstalledModsView, { props: props('cross-remount') });
    await new Promise((r) => setTimeout(r, 50));
    expect(passes('cross-remount')).toBe(1); // joined, not a second network pass
    answer({ status: 'ok', data: { learned: 0 } });
    // …and run once more after it, for the list the second view loaded.
    await waitFor(() => expect(passes('cross-remount')).toBe(2));
  });

  it('a pass that failed changes nothing and breaks nothing', async () => {
    h.learn.mockRejectedValue(new Error('no such command'));
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    render(InstalledModsView, { props: props('cross-failed') });

    await waitFor(() => expect(h.learn).toHaveBeenCalledWith('cross-failed'));
    await waitFor(() => expect(warn).toHaveBeenCalled());
    expect(graphReads('cross-failed')).toBe(1);
    warn.mockRestore();
  });
});
