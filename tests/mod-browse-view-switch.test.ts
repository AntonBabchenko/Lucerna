import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ModSummary, ModVersion } from '$lib/ipc/bindings';

const {
  modsSearch,
  modsGetCurseforgeKeyStatus,
  modsListInstalled,
  modsProject,
  modsProjects,
  modsVersions,
  modsResolveInstallPlan,
  modsInstallWithDeps,
  modsUpdateOne,
  modsUninstall,
  pushSuccess,
  pushWarning,
  pushActionToast,
  dismissToast,
} = vi.hoisted(() => ({
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
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
  pushActionToast: vi.fn(),
  dismissToast: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsSearch,
    modsGetCurseforgeKeyStatus,
    modsListInstalled,
    modsProject,
    modsProjects,
    modsVersions,
    modsResolveInstallPlan,
    modsInstallWithDeps,
    modsUpdateOne,
    modsUninstall,
  },
  events: {
    modInstalled: { listen: vi.fn().mockResolvedValue(() => {}) },
    modUninstalled: { listen: vi.fn().mockResolvedValue(() => {}) },
    modToggle: { listen: vi.fn().mockResolvedValue(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess,
  pushWarning,
  pushActionToast,
  dismiss: dismissToast,
}));

import ModBrowseView from '$lib/mods/ModBrowseView.svelte';
import { modBrowseOpenProject } from '$lib/settings/state.svelte';

const ok = <T>(data: T) => ({ status: 'ok', data }) as const;

function hit(): ModSummary {
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

const V2 = version({ version_id: 'v2', version_number: '2.0', name: 'release-2.0' });
// Same loader, another Minecraft — so `decideModInstall` sees no loader mismatch.
const OLD_MC = version({
  version_id: 'v9',
  version_number: '0.9',
  name: 'release-0.9',
  mc_versions: ['1.19.2'],
});
// Another loader — `decideModInstall` opens the dependency dialog for this one.
const OTHER_LOADER = version({
  version_id: 'v8',
  version_number: '0.8',
  name: 'release-0.8',
  loaders: ['forge'],
});

const installedRow = {
  filename: 'sodium.jar',
  sha1: 'sha-1',
  source: 'modrinth' as const,
  project_id: 'p1',
  version_id: 'v1' as string | null,
  name: 'release-1.0',
  version_number: '1.0',
  installed_at: '2026-06-01T00:00:00Z',
  enabled: true,
};

const REFUSAL = {
  kind: 'mod_version_not_for_instance',
  version_mc: ['1.19.2'],
  version_loaders: ['fabric'],
  instance_mc: '1.20.1',
  instance_loader: 'fabric',
};

const emptyPlan = {
  required: [],
  optional: [],
  incompatible: [],
  unresolvable: [],
  loader_requirements: [],
};

const full = {
  source: 'modrinth',
  instanceId: 'i',
  mcVersion: '1.20.1',
  loader: 'fabric',
} as const;

async function openDrawer() {
  modBrowseOpenProject.value = { source: 'modrinth', projectId: 'p1' };
  render(ModBrowseView, { props: { ...full } });
  const modal = await screen.findByRole('dialog', { name: 'Sodium' });
  await fireEvent.click(within(modal).getByRole('tab', { name: 'Versions' }));
  return modal;
}

beforeEach(() => {
  vi.clearAllMocks();
  modBrowseOpenProject.value = null;
  modsSearch.mockResolvedValue(ok({ hits: [hit()], total: 1, offset: 0, page_size: 20 }));
  modsGetCurseforgeKeyStatus.mockResolvedValue(ok('set'));
  modsListInstalled.mockResolvedValue(ok([]));
  modsProjects.mockResolvedValue(ok([]));
  modsProject.mockResolvedValue(
    ok({
      summary: { ...hit(), slug: 's' },
      description: '',
      website_url: null,
    }),
  );
  modsResolveInstallPlan.mockResolvedValue(ok(emptyPlan));
  modsInstallWithDeps.mockResolvedValue(ok(null));
  modsUpdateOne.mockResolvedValue(ok(null));
  modsUninstall.mockResolvedValue(ok(null));
});

describe('ModBrowseView — switching an installed mod to another version', () => {
  it('goes through ONE update call and never uninstalls first', async () => {
    // Uninstall-then-install as two IPC calls left the user with NO version of
    // the mod whenever the second call failed — a download error was enough.
    modsListInstalled.mockResolvedValue(ok([installedRow]));
    modsVersions.mockResolvedValue(ok([version(), V2]));
    await openDrawer();

    await fireEvent.click(await screen.findByRole('button', { name: 'Switch to this version' }));

    await waitFor(() => expect(modsUpdateOne).toHaveBeenCalledTimes(1));
    expect(modsUpdateOne).toHaveBeenCalledWith(
      'i',
      'sha-1',
      expect.objectContaining({ version_id: 'v2' }),
      false,
    );
    expect(modsUninstall).not.toHaveBeenCalled();
    expect(modsInstallWithDeps).not.toHaveBeenCalled();
    // Like «Update», a switch does not pass through the optional-deps dialog.
    expect(modsResolveInstallPlan).not.toHaveBeenCalled();
  });

  it('recognises the installed build by its bytes when the registry has no version id', async () => {
    modsListInstalled.mockResolvedValue(ok([{ ...installedRow, version_id: null, sha1: 'H' }]));
    // The two builds must differ in their BYTES: the shared `version()` fixture
    // gives every build the same file sha1, and with that both rows would read
    // «installed» — a test that cannot tell "matched by bytes" from "marks
    // everything".
    const otherBytes = version({
      version_id: 'v2',
      version_number: '2.0',
      name: 'release-2.0',
      primary_file: { ...version().primary_file, filename: 'sodium-2.jar', sha1: 'other' },
    });
    modsVersions.mockResolvedValue(ok([version(), otherBytes]));
    const modal = await openDrawer();

    expect(await within(modal).findByText('1.0 · installed')).toBeTruthy();
    expect(within(modal).queryByText('2.0 · installed')).toBeNull();
  });
});

describe('ModBrowseView — a build the platform does not list for this instance', () => {
  beforeEach(() => {
    modsVersions.mockImplementation((_s: unknown, _p: unknown, mc: string | null) =>
      Promise.resolve(ok(mc === null ? [version(), OLD_MC, OTHER_LOADER] : [version()])),
    );
  });

  async function pickForeign(index: number) {
    const modal = await openDrawer();
    await fireEvent.click(within(modal).getByTestId('mod-detail-show-all'));
    await within(modal).findByText('0.9');
    // Row buttons in list order: [v1, v9 (other Minecraft), v8 (other loader)].
    await fireEvent.click(within(modal).getAllByRole('button', { name: 'Install' })[index]);
    await fireEvent.click(await screen.findByRole('button', { name: /install anyway/i }));
  }

  it('installs it with the consent flag once the user agreed in the modal', async () => {
    await pickForeign(1);

    await waitFor(() => expect(modsInstallWithDeps).toHaveBeenCalledTimes(1));
    const [instanceId, ref, extras, allow] = modsInstallWithDeps.mock.calls[0];
    expect(instanceId).toBe('i');
    expect(ref).toEqual({ source: 'modrinth', project_id: 'p1', version_id: 'v9' });
    expect(extras).toEqual([]);
    expect(allow).toBe(true);
  });

  it('keeps the consent when the install passes through the dependency dialog', async () => {
    await pickForeign(2);

    await fireEvent.click(await screen.findByRole('button', { name: /Install \(1 mod\)/ }));

    await waitFor(() => expect(modsInstallWithDeps).toHaveBeenCalledTimes(1));
    expect(modsInstallWithDeps.mock.calls[0][1]).toEqual({
      source: 'modrinth',
      project_id: 'p1',
      version_id: 'v8',
    });
    expect(modsInstallWithDeps.mock.calls[0][3]).toBe(true);
  });

  it('asks instead of toasting when the backend refuses, and a yes re-runs it with consent', async () => {
    // The safety net (D6): the modal could not tell, or was never involved —
    // here, a plain card install. A «Retry» toast could never succeed.
    modsVersions.mockResolvedValue(ok([version()]));
    modsInstallWithDeps
      .mockResolvedValueOnce({ status: 'error', error: REFUSAL })
      .mockResolvedValue(ok(null));
    render(ModBrowseView, { props: { ...full } });

    await fireEvent.click(await screen.findByRole('button', { name: /^install$/i }));

    const dialog = await screen.findByRole('dialog', { name: 'Mod compatibility warning' });
    expect(dialog.textContent).toContain('built for Minecraft 1.19.2, this profile runs 1.20.1');
    expect(pushActionToast).not.toHaveBeenCalled();
    expect(modsInstallWithDeps.mock.calls[0][3]).toBe(false);

    await fireEvent.click(within(dialog).getByRole('button', { name: /install anyway/i }));

    await waitFor(() => expect(modsInstallWithDeps).toHaveBeenCalledTimes(2));
    expect(modsInstallWithDeps.mock.calls[1][3]).toBe(true);
    await waitFor(() =>
      expect(screen.queryByRole('dialog', { name: 'Mod compatibility warning' })).toBeNull(),
    );
  });
});
