// The world picker learns each world's level.dat presence from the library
// listing's `worlds` — the only source for a world the pack is not in yet,
// since such a world has no placement. The picker's own test
// (tests/datapack-world-picker.test.ts) passes `worlds` by hand, and
// svelte-check only proves each mount site passes SOMETHING: `worlds={[]}`
// type-checks. So each mount site is driven here the way a user reaches the
// picker, against a listing whose only knowledge of 'Beta' is its `worlds`
// row, and the row must carry that presence.
//
// The same seam covers what happens when the listing read fails: the picker
// must not open on a stale or empty snapshot, and the failure must be said.

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  DatapackLibraryView,
  InstalledDatapack,
  LibraryInstall,
  ModSummary,
  ModVersion_Serialize,
} from '$lib/ipc/bindings';

const c = vi.hoisted(() => ({
  // Library + picker
  datapacksListLibrary: vi.fn(),
  listWorldNames: vi.fn(),
  datapacksInstallFromFile: vi.fn(),
  datapacksInstallFromVersion: vi.fn(),
  modsDatapackVersions: vi.fn(),
  datapacksCheckUpdates: vi.fn(),
  // Everything else the three surfaces touch on mount
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

const toasts = vi.hoisted(() => ({ success: vi.fn(), warning: vi.fn() }));
vi.mock('$lib/toasts/toasts.svelte', async (importOriginal) => ({
  ...(await importOriginal<typeof import('$lib/toasts/toasts.svelte')>()),
  pushSuccess: (...a: unknown[]) => toasts.success(...a),
  pushWarning: (...a: unknown[]) => toasts.warning(...a),
}));

import AddonsTab from '$lib/mods/AddonsTab.svelte';
import InstalledDatapacksView from '$lib/mods/InstalledDatapacksView.svelte';
import ModBrowseView from '$lib/mods/ModBrowseView.svelte';
import { markSeen } from '$lib/onboarding/contextual-tours';

const pack: InstalledDatapack = {
  filename: 'terralith.zip',
  sha1: 'abc',
  size_bytes: 10,
  pack_format: 48,
  name: 'Terralith',
  source: 'modrinth',
  project_id: 'terra',
  version_id: 'v1',
  version_number: '1.0',
  installed_at: '2026-09-24T00:00:00Z',
};

/** A library in which 'Beta' is known ONLY through `worlds`: the pack is in no
 *  world, so no placement can carry Beta's presence. */
const library: DatapackLibraryView = {
  expected_pack_format: null,
  entries: [{ pack, in_library: true, compat: { kind: 'unknown' }, placements: [] }],
  worlds: [{ world: 'Beta', level_dat: 'only_old' }],
};

const listingFailure = {
  status: 'error' as const,
  error: { kind: 'io' as const, path: '/lib', details: 'locked' },
};

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

beforeEach(() => {
  markSeen('addons');
  c.datapacksListLibrary.mockResolvedValue({ status: 'ok', data: library });
  c.listWorldNames.mockResolvedValue({
    status: 'ok',
    data: [{ folder_name: 'Beta', modified_unix_ms: null }],
  });
  c.datapacksInstallFromFile.mockResolvedValue({ status: 'ok', data: pack });
  c.datapacksInstallFromVersion.mockResolvedValue({ status: 'ok', data: installed });
  c.modsDatapackVersions.mockResolvedValue({ status: 'ok', data: [version] });
  c.datapacksCheckUpdates.mockResolvedValue({ status: 'ok', data: [] });
  c.instanceSupportsDatapacks.mockResolvedValue({ status: 'ok', data: true });
  c.runningInstances.mockResolvedValue([]);
  c.modsSearch.mockResolvedValue({
    status: 'ok',
    data: { hits: [], total: 0, offset: 0, page_size: 20 },
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
  vi.clearAllMocks();
  const s = await import('$lib/settings/state.svelte');
  s.addonsKind.value = 'mod';
  s.datapacksChanged.value = 0;
});

/** The picker row for `world`, once the picker has loaded its worlds. */
async function pickerRow(world: string): Promise<HTMLElement> {
  const boxes = await screen.findAllByTestId('datapack-picker-world');
  const box = boxes.find((b) => b.getAttribute('data-world') === world);
  if (!box) throw new Error(`no picker row for ${world}`);
  return box;
}

describe('the world picker receives the library listing’s worlds at every mount site', () => {
  it('InstalledDatapacksView: «Add to worlds» passes the listing’s worlds', async () => {
    render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
    await fireEvent.click(await screen.findByTestId('datapack-add-to-worlds'));
    const beta = await pickerRow('Beta');
    expect(beta.getAttribute('data-level-dat')).toBe('only_old');
  });

  it('ModBrowseView: the picker after a catalog install gets the listing’s worlds', async () => {
    c.modsSearch.mockResolvedValue({
      status: 'ok',
      data: { hits: [hit], total: 1, offset: 0, page_size: 20 },
    });
    // Not yet in the library, so the card offers an install.
    c.datapacksListLibrary.mockResolvedValue({ status: 'ok', data: { ...library, entries: [] } });
    render(ModBrowseView, {
      props: {
        source: 'modrinth',
        instanceId: 'inst-1',
        mcVersion: '1.21.1',
        loader: 'fabric',
        kind: 'datapack',
      },
    });
    await fireEvent.click(await screen.findByRole('button', { name: /^install$/i }));
    const beta = await pickerRow('Beta');
    expect(beta.getAttribute('data-level-dat')).toBe('only_old');
  });

  it('AddonsTab: the picker after a local install gets the listing’s worlds', async () => {
    render(AddonsTab, { props: { instanceId: 'inst-1', mcVersion: '1.21.1', loader: 'fabric' } });
    await fireEvent.click(await screen.findByRole('tab', { name: /data packs/i }));
    await fireEvent.click(await screen.findByTestId('file-dropzone'));
    const beta = await pickerRow('Beta');
    expect(beta.getAttribute('data-level-dat')).toBe('only_old');
  });
});

describe('a library listing that cannot be read opens no picker and says so', () => {
  it('ModBrowseView: a failed refresh after the install warns instead of opening the picker', async () => {
    c.modsSearch.mockResolvedValue({
      status: 'ok',
      data: { hits: [hit], total: 1, offset: 0, page_size: 20 },
    });
    // The mount-time read succeeds; every read after the install fails.
    c.datapacksListLibrary.mockImplementation(async () =>
      c.datapacksInstallFromVersion.mock.calls.length > 0
        ? listingFailure
        : { status: 'ok', data: { ...library, entries: [] } },
    );
    render(ModBrowseView, {
      props: {
        source: 'modrinth',
        instanceId: 'inst-1',
        mcVersion: '1.21.1',
        loader: 'fabric',
        kind: 'datapack',
      },
    });
    await fireEvent.click(await screen.findByRole('button', { name: /^install$/i }));
    await waitFor(() => {
      expect(toasts.warning).toHaveBeenCalledWith(expect.stringMatching(/IO error at \/lib/i));
    });
    expect(screen.queryByTestId('datapack-world-picker')).toBeNull();
  });

  it('AddonsTab: a failed listing after a local install warns instead of opening the picker', async () => {
    render(AddonsTab, { props: { instanceId: 'inst-1', mcVersion: '1.21.1', loader: 'fabric' } });
    await fireEvent.click(await screen.findByRole('tab', { name: /data packs/i }));
    const zone = await screen.findByTestId('file-dropzone');
    await waitFor(() => expect(c.datapacksListLibrary).toHaveBeenCalled());
    // Only AddonsTab's own read fails. It is the first read after the
    // install: the datapacksChanged bump it makes just before only schedules
    // the embedded browse view's refresh, and that later read succeeds — so
    // a warning here can only be AddonsTab's. (Were the order ever to flip,
    // AddonsTab would get the good listing and open the picker, and this
    // test would fail rather than pass on the browse view's warning.)
    c.datapacksListLibrary.mockResolvedValueOnce(listingFailure);
    await fireEvent.click(zone);
    await waitFor(() => {
      expect(c.datapacksInstallFromFile).toHaveBeenCalled();
      expect(toasts.warning).toHaveBeenCalledWith(expect.stringMatching(/IO error at \/lib/i));
    });
    expect(screen.queryByTestId('datapack-world-picker')).toBeNull();
  });
});
