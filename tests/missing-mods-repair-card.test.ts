import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// The card now reuses the mod-browser install flow: a per-candidate Install
// button runs installCandidate, which calls
//   modsVersions → modsResolveInstallPlan → decideModInstall → modsInstallWithDeps
// (the fast path) or hands off to the DependencyDialog. We mock the IPC
// commands and decideModInstall so the unit test can assert the buttons render
// and the install entry is wired, without standing up the whole flow.

const modsVersions = vi.fn();
const modsResolveInstallPlan = vi.fn();
const modsInstallWithDeps = vi.fn();
const modsProject = vi.fn();
const decideModInstall = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsVersions: (...a: unknown[]) => modsVersions(...a),
    modsResolveInstallPlan: (...a: unknown[]) => modsResolveInstallPlan(...a),
    modsInstallWithDeps: (...a: unknown[]) => modsInstallWithDeps(...a),
    modsProject: (...a: unknown[]) => modsProject(...a),
  },
}));

vi.mock('$lib/mods/dep-prompt', () => ({
  decideModInstall: (...a: unknown[]) => decideModInstall(...a),
}));

vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
}));

import type { ModVersion, RepairPlan, VersionRef } from '$lib/ipc/bindings';
import MissingModsRepairCard from '$lib/logs/MissingModsRepairCard.svelte';

function makeVersionRef(projectId: string): VersionRef {
  return { source: 'modrinth', project_id: projectId, version_id: `v-${projectId}` };
}

function makeModVersion(projectId: string): ModVersion {
  return {
    source: 'modrinth',
    project_id: projectId,
    version_id: `v-${projectId}`,
    name: `${projectId} 1.0.0`,
    version_number: '1.0.0',
    mc_versions: ['1.20.1'],
    loaders: ['forge'],
    primary_file: {
      filename: `${projectId}.jar`,
      url: 'https://example/x.jar',
      sha1: 'aa',
      size: 1,
      distribution_allowed: true,
    },
    deps: [],
    published_at: null,
  };
}

const planWithExactAndUnresolved: Extract<RepairPlan, { kind: 'install_missing_mods' }> = {
  kind: 'install_missing_mods',
  mods: [
    {
      cited: { id: 'create', version: '0.5.1', kind: 'missing' },
      tier: {
        tier: 'exact',
        candidate: {
          target: makeVersionRef('p1'),
          display: {
            source: 'modrinth',
            project_id: 'p1',
            slug: 'create',
            name: 'Create',
            summary: 'A Minecraft mod',
            icon_url: null,
            downloads: 0,
            author: 'simibubi',
            updated_at: null,
          },
          version_label: '0.5.1',
        },
      },
    },
    {
      cited: { id: 'ghostmod', version: null, kind: 'missing' },
      tier: { tier: 'unresolved' },
    },
  ],
};

const planWithFuzzyOnly: Extract<RepairPlan, { kind: 'install_missing_mods' }> = {
  kind: 'install_missing_mods',
  mods: [
    {
      cited: { id: 'somemod', version: '1.0.0', kind: 'missing' },
      tier: {
        tier: 'fuzzy',
        candidates: [
          {
            target: makeVersionRef('cand1'),
            display: {
              source: 'modrinth',
              project_id: 'cand1',
              slug: 'some-mod-a',
              name: 'Some Mod A',
              summary: '',
              icon_url: null,
              downloads: 0,
              author: 'author1',
              updated_at: null,
            },
            version_label: '1.0.0',
          },
          {
            target: makeVersionRef('cand2'),
            display: {
              source: 'modrinth',
              project_id: 'cand2',
              slug: 'some-mod-b',
              name: 'Some Mod B',
              summary: '',
              icon_url: null,
              downloads: 0,
              author: 'author2',
              updated_at: null,
            },
            version_label: '1.0.1',
          },
        ],
      },
    },
  ],
};

const baseProps = {
  instanceId: 'i1',
  mcVersion: '1.20.1',
  loader: 'forge' as const,
};

describe('MissingModsRepairCard', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders a per-candidate Install button for each exact match and lists unresolved mods', () => {
    render(MissingModsRepairCard, {
      props: { plan: planWithExactAndUnresolved, ...baseProps, onClose: vi.fn() },
    });

    // The exact match has its own Install button keyed by project_id.
    const installBtn = screen.getByTestId('missing-install-p1');
    expect(installBtn).toBeTruthy();
    expect((installBtn as HTMLButtonElement).disabled).toBe(false);
    expect(installBtn.textContent).toContain('Install');

    // The unresolved mod keeps its find-it-yourself affordance (plain id row).
    expect(screen.getByTestId('missing-unresolved-ghostmod')).toBeTruthy();
    expect(screen.getByText('ghostmod')).toBeTruthy();
  });

  it('renders a per-candidate Install button for every fuzzy candidate', () => {
    render(MissingModsRepairCard, {
      props: { plan: planWithFuzzyOnly, ...baseProps, onClose: vi.fn() },
    });

    expect(screen.getByTestId('missing-install-cand1')).toBeTruthy();
    expect(screen.getByTestId('missing-install-cand2')).toBeTruthy();
  });

  it('runs the install flow when a candidate Install button is clicked', async () => {
    // Wire the flow so the fast path completes: version found, plan resolved,
    // decision = install, install ok.
    modsVersions.mockResolvedValue({ status: 'ok', data: [makeModVersion('p1')] });
    modsResolveInstallPlan.mockResolvedValue({
      status: 'ok',
      data: {
        required: [],
        optional: [],
        incompatible: [],
        unresolvable: [],
        loader_requirements: [],
      },
    });
    decideModInstall.mockResolvedValue({ kind: 'install', primaryProjectName: 'Create' });
    modsInstallWithDeps.mockResolvedValue({
      status: 'ok',
      data: { primary_name: 'Create', installed_dependencies: [] },
    });

    render(MissingModsRepairCard, {
      props: { plan: planWithExactAndUnresolved, ...baseProps, onClose: vi.fn() },
    });

    await fireEvent.click(screen.getByTestId('missing-install-p1'));

    // The install entry calls the same chain ModBrowseView uses. The flow is
    // multi-await (versions → plan → decide → install), so wait until the
    // terminal call lands rather than asserting on a single microtask turn.
    await vi.waitFor(() => expect(modsInstallWithDeps).toHaveBeenCalled());
    expect(modsVersions).toHaveBeenCalledWith('modrinth', 'p1', '1.20.1', 'forge');
    expect(modsResolveInstallPlan).toHaveBeenCalled();
    expect(decideModInstall).toHaveBeenCalled();
    expect(modsInstallWithDeps).toHaveBeenCalledWith('i1', makeVersionRef('p1'), []);
  });
});
