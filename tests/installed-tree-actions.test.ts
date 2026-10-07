/**
 * The dependency tree's own actions go the Installed row's way (spec 2026-10-07 D3a, D4a): Remove
 * and Disable act on the jar the node stands for through the row's guarded paths (the impact
 * question first), focus stays on the button the user pressed when the node comes back absent,
 * and «show in the list» clears a search that hides the row instead of doing nothing.
 */
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => {
  const mod = (sha1: string, projectId: string, name: string) => ({
    filename: `${name.toLowerCase()}.jar`,
    sha1,
    source: 'modrinth',
    project_id: projectId,
    version_id: 'v',
    name,
    version_number: '1.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled: true,
    enrich_attempted: false,
    requires: [],
  });
  const summary = (projectId: string, name: string) => ({
    source: 'modrinth',
    project_id: projectId,
    slug: name.toLowerCase(),
    name,
    summary: '',
    icon_url: null,
    downloads: 0,
    author: 'x',
    updated_at: null,
  });
  const graphWith = (balmInstalled: boolean) => ({
    roots: [
      {
        sha1: 'a',
        source: 'modrinth',
        project_id: 'PA',
        name: 'Alpha',
        required: [
          {
            source: 'modrinth',
            project_id: 'PBALM',
            name: 'Balm',
            installed: balmInstalled,
            declared: 'required',
            cycle: false,
            children: [],
          },
        ],
        optional: [],
      },
    ],
  });
  const state = {
    rows: [mod('a', 'PA', 'Alpha'), mod('balm-sha', 'PBALM', 'Balm')],
    graph: graphWith(true),
  };
  return {
    mod,
    summary,
    graphWith,
    state,
    listInstalled: vi.fn(() => Promise.resolve({ status: 'ok', data: state.rows })),
    modsDependencyGraph: vi.fn(() => Promise.resolve({ status: 'ok', data: state.graph })),
    modsRemovalImpact: vi.fn(),
    modsUninstall: vi.fn(),
    modsDisable: vi.fn(),
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
        data: [
          ...(ids.includes('PA') ? [h.summary('PA', 'Alpha')] : []),
          ...(ids.includes('PBALM') ? [h.summary('PBALM', 'Balm')] : []),
        ],
      }),
    ),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: h.modsDependencyGraph,
    instanceDependencyPreflight: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { violations: [] } }),
    modsResolveDepNames: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsRemovalImpact: h.modsRemovalImpact,
    modsUninstall: h.modsUninstall,
    modsDisable: h.modsDisable,
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

// A DISTINCT instance id per case: the graph and pre-flight caches are per instance.
const props = (instanceId: string) => ({
  instanceId,
  mcVersion: '1.21.1',
  loader: 'neoforge' as const,
});
const reset = () => {
  h.state.rows = [h.mod('a', 'PA', 'Alpha'), h.mod('balm-sha', 'PBALM', 'Balm')];
  h.state.graph = h.graphWith(true);
  h.modsRemovalImpact.mockReset();
  h.modsUninstall.mockReset();
  h.modsDisable.mockReset();
};
// Alpha's relation cell opens its dependency section.
const openAlphaTree = async () => {
  const pill = await waitFor(() => {
    const el = document.querySelector<HTMLElement>(
      '[data-mod-row="modrinth:PA"] [data-testid="relation-pill"]',
    );
    if (!el) throw new Error('relation pill not rendered yet');
    return el;
  });
  await fireEvent.click(pill);
};

describe('the dependency tree’s actions go the row’s way', () => {
  it('Remove asks the removal impact for the node’s jar, removes it, and keeps focus there', async () => {
    reset();
    h.modsRemovalImpact.mockResolvedValue({ status: 'ok', data: { dependents: [], order: [] } });
    h.modsUninstall.mockImplementation(() => {
      // The jar is gone: the list re-reads without it, the graph comes back with Balm absent.
      h.state.rows = [h.mod('a', 'PA', 'Alpha')];
      h.state.graph = h.graphWith(false);
      return Promise.resolve({
        status: 'ok',
        data: { token: 't', items: [{ sha1: 'balm-sha', name: 'Balm' }] },
      });
    });
    render(InstalledModsView, { props: props('tree-remove') });
    await openAlphaTree();
    const remove = await screen.findByRole('button', { name: 'Remove Balm' });
    remove.focus();

    await fireEvent.click(remove);

    await waitFor(() => expect(h.modsUninstall).toHaveBeenCalledWith('tree-remove', 'balm-sha'));
    expect(h.modsRemovalImpact).toHaveBeenCalledWith('tree-remove', ['balm-sha']);
    // The node stays in Alpha's tree, now absent: the same button offers the install, and focus
    // never left it for the list.
    await waitFor(() => expect(screen.getByRole('button', { name: 'Install Balm' })).toBe(remove));
    expect(document.activeElement).toBe(remove);
  });

  it('Disable switches the node’s enabled jar off through the guarded path', async () => {
    reset();
    h.modsRemovalImpact.mockResolvedValue({
      status: 'ok',
      data: { dependents: [], order: ['balm-sha'] },
    });
    h.modsDisable.mockResolvedValue({ status: 'ok', data: null });
    render(InstalledModsView, { props: props('tree-disable') });
    await openAlphaTree();

    await fireEvent.click(await screen.findByRole('button', { name: 'Disable Balm' }));

    await waitFor(() => expect(h.modsDisable).toHaveBeenCalledWith('tree-disable', 'balm-sha'));
    expect(h.modsRemovalImpact).toHaveBeenCalledWith('tree-disable', ['balm-sha']);
  });

  it('«show in the list» clears a search that hides the row, then shows it', async () => {
    reset();
    render(InstalledModsView, { props: props('tree-locate') });
    await openAlphaTree();
    const search = screen.getByRole('searchbox', { name: 'Filter installed mods' });
    await fireEvent.input(search, { target: { value: 'alp' } });
    await waitFor(() =>
      expect(document.querySelector('[data-mod-row="modrinth:PBALM"]')).toBeNull(),
    );

    await fireEvent.click(await screen.findByRole('button', { name: 'Show Balm in the list' }));

    await waitFor(() =>
      expect(document.querySelector('[data-mod-row="modrinth:PBALM"]')).not.toBeNull(),
    );
    expect((search as HTMLInputElement).value).toBe('');
  });
});
