/**
 * Spec 2026-10-08: after the Installed list loads, the view asks the backend to learn each
 * installed jar's id on the other platform. A pass that learned something makes the dependency
 * graph stale — it is read again (a mod installed from Modrinth is the CurseForge project an addon
 * names); a pass that learned nothing leaves it alone.
 */
import { render, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

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
    graph: vi.fn(() => Promise.resolve({ status: 'ok', data: { roots: [] } })),
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

describe('learning the installed jars’ ids on the other platform', () => {
  it('a pass that learned an id has the dependency graph read again', async () => {
    h.learn.mockResolvedValue({ status: 'ok', data: { learned: 1 } });
    render(InstalledModsView, { props: props('cross-learned') });

    await waitFor(() => expect(h.learn).toHaveBeenCalledWith('cross-learned'));
    await waitFor(() => expect(graphReads('cross-learned')).toBeGreaterThanOrEqual(2));
  });

  it('a pass that learned nothing leaves the graph alone', async () => {
    h.learn.mockResolvedValue({ status: 'ok', data: { learned: 0 } });
    render(InstalledModsView, { props: props('cross-none') });

    await waitFor(() => expect(h.learn).toHaveBeenCalledWith('cross-none'));
    await waitFor(() => expect(graphReads('cross-none')).toBe(1));
    await new Promise((r) => setTimeout(r, 50));
    expect(graphReads('cross-none')).toBe(1);
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
