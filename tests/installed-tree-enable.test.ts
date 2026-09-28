/**
 * A disabled dependency in the tree offers «Enable» (spec 2026-09-28 §6.3). The tree knows the
 * dependency by its project only, so the view looks up the disabled row's jar by (source,
 * project id) and switches it on through the row's own guarded path — the impact question first,
 * then the flip, then a fresh list — never a raw command.
 */
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => {
  const mod = (sha1: string, projectId: string, name: string, enabled: boolean) => ({
    filename: `${name.toLowerCase()}.jar`,
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
  return {
    rows: [mod('a', 'PA', 'Alpha', true), mod('balm-sha', 'PBALM', 'Balm', false)],
    alphaSummary: {
      source: 'modrinth',
      project_id: 'PA',
      slug: 'alpha',
      name: 'Alpha',
      summary: '',
      icon_url: null,
      downloads: 0,
      author: 'x',
      updated_at: null,
    },
    // The graph roots only enabled mods; Balm's jar is there but switched off.
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
              project_id: 'PBALM',
              name: 'Balm',
              installed: false,
              disabled: true,
              declared: 'required',
              cycle: false,
              children: [],
            },
          ],
          optional: [],
        },
      ],
    },
    listInstalled: vi.fn(),
    modsEnableImpact: vi.fn(),
    modsEnable: vi.fn(),
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: h.listInstalled,
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsProjects: vi.fn((_source: string, ids: string[]) =>
      Promise.resolve({ status: 'ok', data: ids.includes('PA') ? [h.alphaSummary] : [] }),
    ),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: h.graph }),
    instanceDependencyPreflight: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { violations: [] } }),
    modsResolveDepNames: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // The guarded enable path (mod-ops) asks for the impact first — WITH its safe flip `order`.
    modsEnableImpact: h.modsEnableImpact,
    modsEnable: h.modsEnable,
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

describe('«Enable» in the dependency tree', () => {
  it('switches the disabled jar on through the guarded path, then re-reads the list', async () => {
    h.listInstalled.mockResolvedValue({ status: 'ok', data: h.rows });
    h.modsEnableImpact.mockResolvedValue({
      status: 'ok',
      data: { requirements: [], order: ['balm-sha'] },
    });
    h.modsEnable.mockResolvedValue({ status: 'ok', data: null });
    render(InstalledModsView, {
      props: { instanceId: 'tree-enable', mcVersion: '1.21.1', loader: 'neoforge' as const },
    });

    const chip = await waitFor(() => {
      const el = document.querySelector<HTMLElement>('[data-testid="dep-expand-chip"]');
      if (!el) throw new Error('dependency chip not rendered yet');
      return el;
    });
    await fireEvent.click(chip);
    const enable = await screen.findByRole('button', { name: 'Enable Balm' });
    const listsBefore = h.listInstalled.mock.calls.length;

    await fireEvent.click(enable);

    await waitFor(() => expect(h.modsEnable).toHaveBeenCalledWith('tree-enable', 'balm-sha'));
    expect(h.modsEnableImpact).toHaveBeenCalledWith('tree-enable', ['balm-sha']);
    await waitFor(() => expect(h.listInstalled.mock.calls.length).toBeGreaterThan(listsBefore));
  });
});
