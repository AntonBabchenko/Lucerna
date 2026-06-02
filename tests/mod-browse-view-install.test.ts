import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ModSummary, ModVersion } from '$lib/ipc/bindings';

// Install + card-action flows of ModBrowseView (the half the existing
// mod-browse-view.test.ts leaves uncovered). Full command mock so the
// version → resolve-plan → install / dependency-dialog path runs. Mocks are
// bare vi.fn() (loose any types) so per-test mockResolvedValue can return
// either ok or error shapes and .mock.calls indexing stays untyped.
const {
  modsSearch,
  modsGetCurseforgeKeyStatus,
  modsListInstalled,
  modsProject,
  modsVersions,
  modsResolveInstallPlan,
  modsInstallWithDeps,
  modsUninstall,
  modsEnable,
  modsDisable,
  pushSuccess,
  pushWarning,
} = vi.hoisted(() => ({
  modsSearch: vi.fn(),
  modsGetCurseforgeKeyStatus: vi.fn(),
  modsListInstalled: vi.fn(),
  modsProject: vi.fn(),
  modsVersions: vi.fn(),
  modsResolveInstallPlan: vi.fn(),
  modsInstallWithDeps: vi.fn(),
  modsUninstall: vi.fn(),
  modsEnable: vi.fn(),
  modsDisable: vi.fn(),
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsSearch,
    modsGetCurseforgeKeyStatus,
    modsListInstalled,
    modsProject,
    modsVersions,
    modsResolveInstallPlan,
    modsInstallWithDeps,
    modsUninstall,
    modsEnable,
    modsDisable,
  },
  events: {
    modInstalled: { listen: vi.fn().mockResolvedValue(() => {}) },
    modUninstalled: { listen: vi.fn().mockResolvedValue(() => {}) },
    modToggle: { listen: vi.fn().mockResolvedValue(() => {}) },
  },
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({ pushSuccess, pushWarning }));

import ModBrowseView from '$lib/mods/ModBrowseView.svelte';

// --- fixtures ---------------------------------------------------------------
function hit(overrides: Partial<ModSummary> = {}): ModSummary {
  return {
    source: 'modrinth',
    project_id: 'p1',
    slug: 'sodium',
    name: 'Sodium',
    summary: '',
    icon_url: null,
    downloads: 1,
    author: '',
    updated_at: null,
    ...overrides,
  };
}

function version(overrides: Partial<ModVersion> = {}): ModVersion {
  return {
    source: 'modrinth',
    project_id: 'p1',
    version_id: 'v1',
    name: 'release-1.0',
    version_number: '1.0',
    mc_versions: ['1.20.1'],
    loaders: ['fabric'],
    primary_file: {
      filename: 'sodium.jar',
      url: '',
      sha1: 'h',
      size: 1,
      distribution_allowed: true,
    },
    deps: [],
    published_at: null,
    ...overrides,
  };
}

const emptyPlan = {
  required: [],
  optional: [],
  incompatible: [],
  unresolvable: [],
  loader_requirements: [],
};

const ok = <T>(data: T) => ({ status: 'ok', data }) as const;
const project = (name: string) =>
  ok({
    summary: {
      source: 'modrinth',
      project_id: 'p1',
      slug: 's',
      name,
      summary: '',
      icon_url: null,
      downloads: 0,
      author: '',
      updated_at: null,
    },
    description: '',
    website_url: null,
  });

function searchReturns(hits: ModSummary[]) {
  modsSearch.mockResolvedValue(ok({ hits, total: hits.length, offset: 0, page_size: 20 }));
}

const full = {
  source: 'modrinth',
  instanceId: 'i',
  mcVersion: '1.20.1',
  loader: 'fabric',
} as const;

beforeEach(() => {
  vi.clearAllMocks();
  // Baseline defaults — individual tests override the relevant ones. A default
  // empty page for modsSearch is the safety net: a test that forgets
  // searchReturns() still renders (no card) instead of an undefined result
  // crashing the fill loop.
  modsSearch.mockResolvedValue(ok({ hits: [], total: 0, offset: 0, page_size: 20 }));
  modsGetCurseforgeKeyStatus.mockResolvedValue(ok('set'));
  modsListInstalled.mockResolvedValue(ok([]));
  modsProject.mockResolvedValue(project('Sodium'));
  modsInstallWithDeps.mockResolvedValue(ok(null));
  modsUninstall.mockResolvedValue(ok(null));
  modsEnable.mockResolvedValue(ok(null));
  modsDisable.mockResolvedValue(ok(null));
});

describe('ModBrowseView install flow', () => {
  it('installs directly when the resolve plan has no extras', async () => {
    searchReturns([hit()]);
    modsVersions.mockResolvedValue(ok([version()]));
    modsResolveInstallPlan.mockResolvedValue(ok(emptyPlan));
    render(ModBrowseView, { props: { ...full } });

    await fireEvent.click(await screen.findByRole('button', { name: /^install$/i }));

    await waitFor(() => expect(modsInstallWithDeps).toHaveBeenCalledTimes(1));
    const [instanceId, ref, extras] = modsInstallWithDeps.mock.calls[0];
    expect(instanceId).toBe('i');
    expect(ref).toEqual({ source: 'modrinth', project_id: 'p1', version_id: 'v1' });
    expect(extras).toEqual([]);
    await waitFor(() => expect(pushSuccess).toHaveBeenCalled());
    expect(pushWarning).not.toHaveBeenCalled();
    // Fast path, not the dialog branch — no "Install (N mods)" confirm appears.
    expect(screen.queryByRole('button', { name: /Install \(/ })).toBeNull();
  });

  it('warns with the failure detail when the direct install fails', async () => {
    searchReturns([hit()]);
    modsVersions.mockResolvedValue(ok([version()]));
    modsResolveInstallPlan.mockResolvedValue(ok(emptyPlan));
    modsInstallWithDeps.mockResolvedValue({
      status: 'error',
      error: { kind: 'mods_cache_io', details: 'disk full' },
    });
    render(ModBrowseView, { props: { ...full } });

    await fireEvent.click(await screen.findByRole('button', { name: /^install$/i }));

    await waitFor(() => expect(pushWarning).toHaveBeenCalled());
    // Title + the formatted error detail line both reach the toast.
    expect(pushWarning).toHaveBeenCalledWith(
      expect.any(String),
      expect.arrayContaining([expect.stringContaining('disk full')]),
    );
    expect(pushSuccess).not.toHaveBeenCalled();
  });

  it('shows an error when no compatible version exists', async () => {
    searchReturns([hit()]);
    modsVersions.mockResolvedValue(ok([]));
    render(ModBrowseView, { props: { ...full } });

    await fireEvent.click(await screen.findByRole('button', { name: /^install$/i }));

    await waitFor(() => expect(modsVersions).toHaveBeenCalled());
    expect(await screen.findByText('No compatible version found')).toBeTruthy();
    expect(modsInstallWithDeps).not.toHaveBeenCalled();
  });

  it('surfaces a version-lookup error and does not install', async () => {
    searchReturns([hit()]);
    modsVersions.mockResolvedValue({
      status: 'error',
      error: { kind: 'mods_network', url: 'https://api', details: 'timeout' },
    });
    render(ModBrowseView, { props: { ...full } });

    await fireEvent.click(await screen.findByRole('button', { name: /^install$/i }));

    await waitFor(() => expect(modsVersions).toHaveBeenCalled());
    expect(await screen.findByText(/timeout/)).toBeTruthy();
    expect(modsInstallWithDeps).not.toHaveBeenCalled();
  });

  it('opens the dependency dialog when the plan has required deps', async () => {
    searchReturns([hit()]);
    modsVersions.mockResolvedValue(ok([version()]));
    modsResolveInstallPlan.mockResolvedValue(
      ok({ ...emptyPlan, required: [version({ project_id: 'dep1', version_id: 'dv1' })] }),
    );
    render(ModBrowseView, { props: { ...full } });

    await fireEvent.click(await screen.findByRole('button', { name: /^install$/i }));

    // DependencyDialog renders an "Install (N mods)" confirm = primary + 1 dep.
    expect(await screen.findByRole('button', { name: /Install \(2 mods\)/ })).toBeTruthy();
    // The dialog defers to its own confirm — the direct install path is skipped.
    expect(modsInstallWithDeps).not.toHaveBeenCalled();
  });

  it('blocks install with a clear error when no instance is active', async () => {
    searchReturns([hit()]);
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: null, mcVersion: null, loader: null },
    });

    await fireEvent.click(await screen.findByRole('button', { name: /^install$/i }));

    expect(await screen.findByText('No active instance')).toBeTruthy();
    expect(modsVersions).not.toHaveBeenCalled();
  });
});

describe('ModBrowseView card actions', () => {
  const installedRow = {
    filename: 'sodium.jar',
    sha1: 'sha-1',
    source: 'modrinth' as const,
    project_id: 'p1',
    version_id: 'v1',
    name: 'release-1.0',
    version_number: '1.0',
    installed_at: '2026-06-01T00:00:00Z',
    enabled: true,
  };

  it('uninstalls an installed card', async () => {
    searchReturns([hit()]);
    modsListInstalled.mockResolvedValue(ok([installedRow]));
    render(ModBrowseView, { props: { ...full } });

    await fireEvent.click(await screen.findByRole('button', { name: /uninstall/i }));

    await waitFor(() => expect(modsUninstall).toHaveBeenCalledWith('i', 'sha-1'));
  });

  it('disables an installed-and-enabled card', async () => {
    searchReturns([hit()]);
    modsListInstalled.mockResolvedValue(ok([installedRow]));
    render(ModBrowseView, { props: { ...full } });

    await fireEvent.click(await screen.findByRole('button', { name: /disable/i }));

    await waitFor(() => expect(modsDisable).toHaveBeenCalledWith('i', 'sha-1'));
    expect(modsEnable).not.toHaveBeenCalled();
  });
});
