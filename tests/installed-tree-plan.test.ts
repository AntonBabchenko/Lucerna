/**
 * A version mismatch in the dependency tree offers «Fix…» (spec §6.5): the tree knows the
 * dependency by its project only, so the view names the conflict behind the mark — the one the
 * node's own dependent declared — and asks the same planner as the panel and the row, whose offers
 * show in the «What stops the game» row.
 */
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => {
  const mod = (sha1: string, projectId: string, name: string) => ({
    filename: `${name.toLowerCase()}.jar`,
    sha1,
    source: 'modrinth',
    project_id: projectId,
    version_id: `v-${sha1}`,
    name,
    version_number: '1.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled: true,
    enrich_attempted: false,
    requires: [],
  });
  return {
    rows: [mod('ind', 'PIND', 'Indium'), mod('sod', 'PSOD', 'Sodium')],
    indiumSummary: {
      source: 'modrinth',
      project_id: 'PIND',
      slug: 'indium',
      name: 'Indium',
      summary: '',
      icon_url: null,
      downloads: 0,
      author: 'x',
      updated_at: null,
    },
    // Indium requires Sodium, which is installed — in a version Indium's range rejects.
    graph: {
      roots: [
        {
          sha1: 'ind',
          source: 'modrinth',
          project_id: 'PIND',
          name: 'Indium',
          required: [
            {
              source: 'modrinth',
              project_id: 'PSOD',
              name: 'Sodium',
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
    conflict: {
      dependent_sha1: 'ind',
      dependent_name: 'Indium',
      dep_id: 'sodium',
      kind: 'version_out_of_range',
      installed_version: '0.6.0',
      needed: '0.5.x',
      needed_desc: {
        raw: '0.5.x',
        family: 'fabric_predicate',
        alternatives: [],
        unparseable: true,
        soft: false,
      },
      provider_project: { source: 'modrinth', project_id: 'PSOD', version_id: null },
      provider_sha1: 'sod',
      provider_name: 'Sodium',
      family: 'fabric_predicate',
    },
    modsPlanVersionFix: vi.fn(),
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: h.rows }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsProjects: vi.fn((_source: string, ids: string[]) =>
      Promise.resolve({ status: 'ok', data: ids.includes('PIND') ? [h.indiumSummary] : [] }),
    ),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: h.graph }),
    instanceDependencyPreflight: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { violations: [h.conflict] } }),
    modsResolveDepNames: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsPlanVersionFix: h.modsPlanVersionFix,
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

describe('«Fix…» on a version mismatch in the dependency tree', () => {
  it('asks the planner about the conflict Indium declared, and the panel shows the offers', async () => {
    h.modsPlanVersionFix.mockResolvedValue({
      status: 'ok',
      data: {
        update_dependent: null,
        change_provider: {
          version: { version_number: '0.5.11', name: 'Sodium 0.5.11' },
          direction: 'downgrade',
          breaks: [],
        },
      },
    });
    render(InstalledModsView, {
      props: { instanceId: 'tree-plan', mcVersion: '1.21.1', loader: 'fabric' as const },
    });

    const chip = await waitFor(() => {
      const el = document.querySelector<HTMLElement>('[data-testid="relation-pill"]');
      if (!el) throw new Error('relation pill not rendered yet');
      return el;
    });
    await fireEvent.click(chip);
    const fix = await screen.findByRole('button', { name: 'Fix the version conflict with Sodium' });
    await fireEvent.click(fix);

    expect(h.modsPlanVersionFix).toHaveBeenCalledWith('tree-plan', 'ind', 'sodium');
    const panel = screen.getByTestId('preflight-panel');
    const offer = await within(panel).findByTestId('preflight-plan-provider');
    expect(offer.textContent?.trim()).toBe('Roll Sodium back to 0.5.11');
  });
});
