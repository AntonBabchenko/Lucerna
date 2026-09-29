/**
 * A version conflict belongs to the mod that declared the range (plan §5b V1, screenshot 06d).
 * Indium's range rejects the installed Sodium; Iris is fine with it. Both trees show Sodium, and
 * only Indium's may say «version mismatch» and offer «Fix…» — the mark is per EDGE (this
 * dependent → this dependency), never per project. «Fix…» there plans Indium's own conflict.
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
  const sodiumNode = {
    source: 'modrinth',
    project_id: 'PSOD',
    name: 'Sodium',
    installed: true,
    declared: 'required',
    cycle: false,
    children: [],
  };
  const root = (sha1: string, projectId: string, name: string) => ({
    sha1,
    source: 'modrinth',
    project_id: projectId,
    name,
    required: [sodiumNode],
    optional: [],
  });
  return {
    rows: [mod('ind', 'PIND', 'Indium'), mod('iri', 'PIRI', 'Iris'), mod('sod', 'PSOD', 'Sodium')],
    summaries: [summary('PIND', 'Indium'), summary('PIRI', 'Iris'), summary('PSOD', 'Sodium')],
    // Both require Sodium, which is installed; only Indium's range rejects it.
    graph: { roots: [root('ind', 'PIND', 'Indium'), root('iri', 'PIRI', 'Iris')] },
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
      Promise.resolve({
        status: 'ok',
        data: h.summaries.filter((s) => ids.includes(s.project_id)),
      }),
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

/** Open a row's dependency section and hand back the tree under «Requires». */
async function openTree(modName: string): Promise<HTMLElement> {
  const row = await screen.findByRole('group', { name: modName });
  const pill = await waitFor(() => {
    const el = row.querySelector<HTMLElement>('[data-testid="relation-pill"]');
    if (!el) throw new Error(`${modName}: relation pill not rendered yet`);
    return el;
  });
  await fireEvent.click(pill);
  return within(row).findByRole('tree');
}

const sodiumIn = (tree: HTMLElement) => within(tree).getByRole('treeitem', { name: 'Sodium' });

describe('a version conflict marks the dependency only under the mod that declared it', () => {
  it("Iris's tree shows Sodium installed while Indium's shows the mismatch", async () => {
    render(InstalledModsView, {
      props: { instanceId: 'edge-conflict', mcVersion: '1.21.1', loader: 'fabric' as const },
    });
    const indium = await openTree('Indium');
    await waitFor(() =>
      expect(sodiumIn(indium).getAttribute('data-node-state')).toBe('out_of_range'),
    );
    const iris = await openTree('Iris');

    expect(sodiumIn(iris).getAttribute('data-node-state')).toBe('installed');
    expect(within(iris).queryByText('version mismatch')).toBeNull();
    expect(within(iris).queryByRole('button', { name: /version conflict/i })).toBeNull();
    expect(within(indium).getByText('version mismatch')).toBeTruthy();
  });

  it("«Fix…» under Indium plans Indium's own conflict", async () => {
    h.modsPlanVersionFix.mockResolvedValue({
      status: 'ok',
      data: { update_dependent: null, change_provider: null },
    });
    render(InstalledModsView, {
      props: { instanceId: 'edge-conflict-fix', mcVersion: '1.21.1', loader: 'fabric' as const },
    });
    const indium = await openTree('Indium');
    const fix = await within(indium).findByRole('button', {
      name: 'Fix the version conflict with Sodium',
    });
    await fireEvent.click(fix);
    expect(h.modsPlanVersionFix).toHaveBeenCalledWith('edge-conflict-fix', 'ind', 'sodium');
  });
});
