// A data pack dialog belongs to the instance it was opened for. The library
// screen already closes its own on an instance switch
// (tests/installed/installed-datapacks-instance-switch.test.ts); the catalog
// and the Add-ons tab hold dialogs of their own. Left open across a switch,
// their Confirm would act on the NEW instance with the old instance's pack
// name and world list: a cascade removal of a same-named pack, say.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  DatapackLibraryView,
  InstalledDatapack,
  LibraryInstall,
  ModSummary,
  ModVersion_Serialize,
} from '$lib/ipc/bindings';

const c = vi.hoisted(() => ({
  datapacksListLibrary: vi.fn(),
  datapacksWorldEntryKind: vi.fn(),
  listWorldNames: vi.fn(),
  datapacksInstallFromFile: vi.fn(),
  datapacksInstallFromVersion: vi.fn(),
  modsDatapackVersions: vi.fn(),
  datapacksCheckUpdates: vi.fn(),
  instanceSupportsDatapacks: vi.fn(),
  runningInstances: vi.fn(),
  modsSearch: vi.fn(),
  modsProjects: vi.fn(),
  modsGetCurseforgeKeyStatus: vi.fn(),
  modsListInstalled: vi.fn(),
  modsPackOriginSummary: vi.fn(),
  modsDependencyGraph: vi.fn(),
  instanceDependencyPreflight: vi.fn(),
  scanInstanceModCompat: vi.fn(),
  checkInstanceModCompat: vi.fn(),
  assetsList: vi.fn(),
  assetsCheckUpdates: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: c,
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
    processSpawned: { listen: () => Promise.resolve(() => {}) },
    processExited: { listen: () => Promise.resolve(() => {}) },
  },
}));

const openMock = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: (...a: unknown[]) => openMock(...a) }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn().mockResolvedValue(undefined) }));

import AddonsTab from '$lib/mods/AddonsTab.svelte';
import ModBrowseView from '$lib/mods/ModBrowseView.svelte';
import { markSeen } from '$lib/onboarding/contextual-tours';
import { dismiss, toastList } from '$lib/toasts/toasts.svelte';

const pack: InstalledDatapack = {
  filename: 'terralith.zip',
  sha1: 'abc',
  size_bytes: 10,
  name: 'Terralith',
  source: 'modrinth',
  project_id: 'terra',
  version_id: 'v1',
  version_number: '1.0',
  installed_at: '2026-09-24T00:00:00Z',
};

const library: DatapackLibraryView = {
  entries: [
    {
      pack,
      in_library: true,
      compat: { kind: 'unknown' },
      placements: [{ world: 'Beta', state: 'enabled', ignored_reason: null, level_dat: 'present' }],
    },
  ],
  worlds: [{ world: 'Beta', level_dat: 'present' }],
};
const emptyLibrary: DatapackLibraryView = { entries: [], worlds: library.worlds };

const hit: ModSummary = {
  source: 'modrinth',
  project_id: 'terra',
  slug: 'terralith',
  name: 'Terralith',
  summary: '',
  icon_url: null,
  downloads: 1,
  author: '',
  updated_at: null,
};

const version: ModVersion_Serialize = {
  source: 'modrinth',
  project_id: 'terra',
  version_id: 'v1',
  name: 'Terralith 1.0',
  version_number: '1.0',
  mc_versions: ['1.21.1'],
  loaders: [],
  primary_file: {
    filename: 'terralith.zip',
    url: 'https://cdn.modrinth.com/terralith.zip',
    sha1: 'abc',
    size: 10,
    distribution_allowed: true,
  },
  deps: [],
  published_at: null,
};

const installed: LibraryInstall = { pack, refreshed: [] };

const browseProps = {
  source: 'modrinth' as const,
  instanceId: 'inst-1',
  mcVersion: '1.21.1',
  loader: 'fabric' as const,
  kind: 'datapack' as const,
};
const addonsProps = { instanceId: 'inst-1', mcVersion: '1.21.1', loader: 'fabric' as const };

beforeEach(() => {
  markSeen('addons');
  c.datapacksListLibrary.mockResolvedValue({ status: 'ok', data: library });
  c.datapacksWorldEntryKind.mockResolvedValue({ status: 'ok', data: { kind: 'library_copy' } });
  c.listWorldNames.mockResolvedValue({
    status: 'ok',
    data: [{ folder_name: 'Beta', modified_unix_ms: null }],
  });
  c.datapacksInstallFromFile.mockResolvedValue({ status: 'ok', data: installed });
  c.datapacksInstallFromVersion.mockResolvedValue({ status: 'ok', data: installed });
  c.modsDatapackVersions.mockResolvedValue({ status: 'ok', data: [version] });
  c.datapacksCheckUpdates.mockResolvedValue({ status: 'ok', data: [] });
  c.instanceSupportsDatapacks.mockResolvedValue({ status: 'ok', data: true });
  c.runningInstances.mockResolvedValue([]);
  c.modsSearch.mockResolvedValue({
    status: 'ok',
    data: { hits: [hit], total: 1, offset: 0, page_size: 20 },
  });
  c.modsProjects.mockResolvedValue({ status: 'ok', data: [] });
  c.modsGetCurseforgeKeyStatus.mockResolvedValue({ status: 'ok', data: 'set' });
  c.modsListInstalled.mockResolvedValue({ status: 'ok', data: [] });
  c.modsPackOriginSummary.mockResolvedValue({ status: 'ok', data: null });
  c.modsDependencyGraph.mockResolvedValue({ status: 'ok', data: { roots: [] } });
  c.instanceDependencyPreflight.mockResolvedValue({ status: 'ok', data: { violations: [] } });
  c.scanInstanceModCompat.mockResolvedValue({ status: 'ok', data: [] });
  c.checkInstanceModCompat.mockResolvedValue({ status: 'ok', data: [] });
  c.assetsList.mockResolvedValue({ status: 'ok', data: [] });
  c.assetsCheckUpdates.mockResolvedValue({ status: 'ok', data: [] });
  openMock.mockResolvedValue(['/x/terralith.zip']);
});

afterEach(async () => {
  for (const x of toastList()) dismiss(x.id);
  vi.clearAllMocks();
  const s = await import('$lib/settings/state.svelte');
  s.addonsKind.value = 'mod';
  s.datapacksChanged.value = 0;
});

type InstallResult = { status: 'ok'; data: LibraryInstall };

/** An install that stays in flight until the test lets it finish. */
function pendingInstall(): { promise: Promise<InstallResult>; finish: () => void } {
  let finish = (): void => {};
  const promise = new Promise<InstallResult>((resolve) => {
    finish = () => resolve({ status: 'ok', data: installed });
  });
  return { promise, finish: () => finish() };
}

/** Lets every pending Svelte update and queued promise settle. */
async function flush(): Promise<void> {
  await tick();
  await new Promise((r) => setTimeout(r, 0));
  await tick();
}

describe('ModBrowseView — switching instance', () => {
  it('closes the removal dialog opened from a card', async () => {
    const r = render(ModBrowseView, { props: browseProps });
    await fireEvent.click(await screen.findByRole('button', { name: /^remove$/i }));
    expect(await screen.findByTestId('datapack-remove-dialog')).toBeTruthy();

    await r.rerender({ ...browseProps, instanceId: 'inst-2' });
    await waitFor(() => expect(screen.queryByTestId('datapack-remove-dialog')).toBeNull());
  });

  it('closes the world picker opened after an install', async () => {
    c.datapacksListLibrary.mockImplementation(async () => ({
      status: 'ok',
      data: c.datapacksInstallFromVersion.mock.calls.length > 0 ? library : emptyLibrary,
    }));
    const r = render(ModBrowseView, { props: browseProps });
    await fireEvent.click(await screen.findByRole('button', { name: /^install$/i }));
    expect(await screen.findByTestId('datapack-world-picker')).toBeTruthy();

    await r.rerender({ ...browseProps, instanceId: 'inst-2' });
    await waitFor(() => expect(screen.queryByTestId('datapack-world-picker')).toBeNull());
  });

  it('opens no picker when the instance changed while the install ran', async () => {
    const install = pendingInstall();
    c.datapacksListLibrary.mockImplementation(async () => ({
      status: 'ok',
      data: c.datapacksInstallFromVersion.mock.calls.length > 0 ? library : emptyLibrary,
    }));
    c.datapacksInstallFromVersion.mockReturnValue(install.promise);
    const r = render(ModBrowseView, { props: browseProps });
    await fireEvent.click(await screen.findByRole('button', { name: /^install$/i }));
    await waitFor(() =>
      expect(c.datapacksInstallFromVersion).toHaveBeenCalledWith('inst-1', version),
    );

    await r.rerender({ ...browseProps, instanceId: 'inst-2' });
    install.finish();
    await flush();
    await flush();
    expect(screen.queryByTestId('datapack-world-picker')).toBeNull();
  });
});

describe('AddonsTab — switching instance', () => {
  async function installLocally(): Promise<void> {
    await fireEvent.click(await screen.findByRole('tab', { name: /data packs/i }));
    await fireEvent.click(await screen.findByTestId('file-dropzone'));
  }

  it('closes the world picker opened after a local install', async () => {
    const r = render(AddonsTab, { props: addonsProps });
    await installLocally();
    expect(await screen.findByTestId('datapack-world-picker')).toBeTruthy();

    await r.rerender({ ...addonsProps, instanceId: 'inst-2' });
    await waitFor(() => expect(screen.queryByTestId('datapack-world-picker')).toBeNull());
  });

  it('opens no picker when the instance changed while the install ran', async () => {
    const install = pendingInstall();
    c.datapacksInstallFromFile.mockReturnValue(install.promise);
    const r = render(AddonsTab, { props: addonsProps });
    await installLocally();
    await waitFor(() =>
      expect(c.datapacksInstallFromFile).toHaveBeenCalledWith('inst-1', '/x/terralith.zip'),
    );

    await r.rerender({ ...addonsProps, instanceId: 'inst-2' });
    install.finish();
    await flush();
    await flush();
    expect(screen.queryByTestId('datapack-world-picker')).toBeNull();
  });
});
