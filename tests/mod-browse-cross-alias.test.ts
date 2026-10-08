/**
 * Spec 2026-10-08 aliases-everywhere D3: Browse knows a mod installed from the other platform by
 * the id it learned there. Sodium is installed from Modrinth (`p1`); its CurseForge card (394468)
 * is that mod — its badge, its detail modal's installed build and its version switch all act on
 * the Modrinth jar. The browser also runs the learning pass itself: the Installed pane mounts
 * lazily, so a user who only browses would never get the alias otherwise.
 */
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ModSummary, ModVersion } from '$lib/ipc/bindings';

const m = vi.hoisted(() => ({
  modsSearch: vi.fn(),
  modsGetCurseforgeKeyStatus: vi.fn(),
  modsListInstalled: vi.fn(),
  modsProject: vi.fn(),
  modsProjects: vi.fn(),
  modsVersions: vi.fn(),
  modsResolveInstallPlan: vi.fn(),
  modsInstallWithDeps: vi.fn(),
  modsUpdateOne: vi.fn(),
  modsUninstall: vi.fn(),
  modsCrossAliases: vi.fn(),
  modsLearnCrossIds: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: m,
  events: {
    modInstalled: { listen: vi.fn().mockResolvedValue(() => {}) },
    modUninstalled: { listen: vi.fn().mockResolvedValue(() => {}) },
    modToggle: { listen: vi.fn().mockResolvedValue(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
  pushActionToast: vi.fn(),
  dismiss: vi.fn(),
}));

import { resetCrossIdsForTests } from '$lib/mods/cross-ids.svelte';
import ModBrowseView from '$lib/mods/ModBrowseView.svelte';
import { modBrowseOpenProject } from '$lib/settings/state.svelte';

const ok = <T>(data: T) => ({ status: 'ok', data }) as const;

// The CurseForge card. Its name shares nothing with the installed row's (no project names are
// resolved here), so only the alias can match it.
function cfHit(): ModSummary {
  return {
    source: 'curseforge',
    project_id: '394468',
    slug: 'sodium',
    name: 'Sodium',
    summary: '',
    icon_url: null,
    downloads: 1,
    author: '',
    updated_at: null,
  };
}

function cfVersion(overrides: Partial<ModVersion> = {}): ModVersion {
  return {
    source: 'curseforge',
    project_id: '394468',
    version_id: '5000001',
    name: 'sodium-fabric-1.0.jar',
    version_number: '1.0',
    mc_versions: ['1.20.1'],
    loaders: ['fabric'],
    primary_file: {
      filename: 'sodium-fabric-1.0.jar',
      url: '',
      sha1: 'sha-1', // the very bytes installed from Modrinth
      size: 1,
      distribution_allowed: true,
    },
    deps: [],
    published_at: null,
    ...overrides,
  };
}

const modrinthRow = {
  filename: 'sodium-fabric-1.0.jar',
  sha1: 'sha-1',
  source: 'modrinth' as const,
  project_id: 'p1',
  version_id: 'v1',
  name: 'Sodium 1.0',
  version_number: '1.0',
  installed_at: '2026-06-01T00:00:00Z',
  enabled: true,
  enrich_attempted: false,
  requires: [],
};

const alias = {
  alias_source: 'curseforge',
  alias_project_id: '394468',
  own_source: 'modrinth',
  own_project_id: 'p1',
};

const props = {
  source: 'curseforge',
  instanceId: 'cross-i',
  mcVersion: '1.20.1',
  loader: 'fabric',
} as const;

const settle = async () => {
  for (let i = 0; i < 6; i++) await new Promise((r) => setTimeout(r, 0));
};

beforeEach(() => {
  vi.clearAllMocks();
  resetCrossIdsForTests();
  modBrowseOpenProject.value = null;
  m.modsSearch.mockResolvedValue(ok({ hits: [cfHit()], total: 1, offset: 0, page_size: 20 }));
  m.modsGetCurseforgeKeyStatus.mockResolvedValue(ok('set'));
  m.modsListInstalled.mockResolvedValue(ok([modrinthRow]));
  m.modsProjects.mockResolvedValue(ok([]));
  m.modsProject.mockResolvedValue(
    ok({ summary: { ...cfHit(), slug: 's' }, description: '', website_url: null }),
  );
  m.modsResolveInstallPlan.mockResolvedValue(
    ok({ required: [], optional: [], incompatible: [], unresolvable: [], loader_requirements: [] }),
  );
  m.modsInstallWithDeps.mockResolvedValue(ok(null));
  m.modsUpdateOne.mockResolvedValue(ok(null));
  m.modsCrossAliases.mockResolvedValue(ok([alias]));
  m.modsLearnCrossIds.mockResolvedValue(ok({ learned: 0 }));
});

describe('Browse and a mod installed from the other platform', () => {
  it('reads the CurseForge card installed through its alias, and runs the learning pass', async () => {
    render(ModBrowseView, { props });
    await settle();
    await waitFor(() => expect(screen.getByText(/Installed/)).toBeTruthy());
    expect(screen.queryByRole('button', { name: /^install$/i })).toBeNull();
    expect(m.modsLearnCrossIds).toHaveBeenCalledWith('cross-i');
  });

  it('without the alias the card is not installed (no name in common)', async () => {
    m.modsCrossAliases.mockResolvedValue(ok([]));
    render(ModBrowseView, { props });
    await settle();
    expect(screen.queryByText(/Installed/)).toBeNull();
  });

  it('its detail modal marks the installed build, and a switch replaces the Modrinth jar', async () => {
    const v2 = cfVersion({
      version_id: '5000002',
      version_number: '2.0',
      name: 'sodium-fabric-2.0.jar',
      primary_file: { ...cfVersion().primary_file, filename: 'sodium-fabric-2.0.jar', sha1: 'new' },
    });
    m.modsVersions.mockResolvedValue(ok([cfVersion(), v2]));
    modBrowseOpenProject.value = { source: 'curseforge', projectId: '394468' };
    render(ModBrowseView, { props });
    await settle();
    const modal = await screen.findByRole('dialog', { name: 'Sodium' });
    await fireEvent.click(within(modal).getByRole('tab', { name: 'Versions' }));

    // The same bytes on CurseForge are the installed build — not something to install again.
    expect(await within(modal).findByText('1.0 · installed')).toBeTruthy();
    await fireEvent.click(await screen.findByRole('button', { name: 'Switch to this version' }));
    await waitFor(() => expect(m.modsUpdateOne).toHaveBeenCalledTimes(1));
    expect(m.modsUpdateOne).toHaveBeenCalledWith(
      'cross-i',
      'sha-1',
      expect.objectContaining({ version_id: '5000002' }),
      false,
    );
    expect(m.modsInstallWithDeps).not.toHaveBeenCalled();
  });
});
