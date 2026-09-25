// An update that left worlds behind must say only what is true about a retry.
// A renamed update keeps the old library copy (`old_copy_kept`), so a retry
// can still move those worlds, and the warning says so. A same-name update
// replaced the library copy in place: a retry would find each stale world's
// copy foreign and skip it. That one gets the reinstall wording, which
// promises nothing, and the update badge goes, because the library is already
// on the new version.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  DatapackLibraryView,
  DatapackUpdateOutcome,
  InstalledDatapack,
  ModSummary,
  ModVersion_Serialize,
} from '$lib/ipc/bindings';

const c = vi.hoisted(() => ({
  datapacksListLibrary: vi.fn(),
  datapacksCheckUpdates: vi.fn(),
  datapacksUpdateOne: vi.fn(),
  modsDatapackVersions: vi.fn(),
  modsProject: vi.fn(),
  runningInstances: vi.fn(),
  modsSearch: vi.fn(),
  modsProjects: vi.fn(),
  modsGetCurseforgeKeyStatus: vi.fn(),
  modsListInstalled: vi.fn(),
  assetsList: vi.fn(),
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
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn().mockResolvedValue(undefined) }));

import InstalledDatapacksView from '$lib/mods/InstalledDatapacksView.svelte';
import ModBrowseView from '$lib/mods/ModBrowseView.svelte';
import { dismiss, toastList } from '$lib/toasts/toasts.svelte';

const pack: InstalledDatapack = {
  filename: 'vm.zip',
  sha1: 'a'.repeat(40),
  size_bytes: 10,
  name: 'VeinMiner',
  source: 'modrinth',
  project_id: 'vm',
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
      placements: [
        { world: 'Alpha', state: 'enabled', ignored_reason: null, level_dat: 'present' },
      ],
    },
  ],
  worlds: [{ world: 'Alpha', level_dat: 'present' }],
};

function version(id: string, n: string): ModVersion_Serialize {
  return {
    source: 'modrinth',
    project_id: 'vm',
    version_id: id,
    name: `VeinMiner ${n}`,
    version_number: n,
    mc_versions: ['1.21.1'],
    loaders: [],
    primary_file: {
      filename: 'vm.zip',
      url: 'https://cdn.modrinth.com/vm.zip',
      sha1: 'b',
      size: 10,
      distribution_allowed: true,
    },
    deps: [],
    published_at: null,
  };
}
const hit: ModSummary = {
  source: 'modrinth',
  project_id: 'vm',
  slug: 'vm',
  name: 'VeinMiner',
  summary: '',
  icon_url: null,
  downloads: 1,
  author: '',
  updated_at: null,
};

const V1 = version('v1', '1.0');
const V2 = version('v2', '2.0');

/** Alpha could not be moved to the new version. */
function outcome(oldCopyKept: boolean): DatapackUpdateOutcome {
  return {
    pack: { ...pack, version_id: 'v2', version_number: '2.0' },
    migrations: [{ kind: 'failed', world: 'Alpha', details: 'locked' }],
    completed: false,
    old_copy_kept: oldCopyKept,
  };
}

function warnings() {
  return toastList().filter((x) => x.kind === 'warning');
}

beforeEach(() => {
  c.datapacksListLibrary.mockResolvedValue({ status: 'ok', data: library });
  c.datapacksCheckUpdates.mockResolvedValue({
    status: 'ok',
    data: [
      { filename: 'vm.zip', name: 'VeinMiner', state: { kind: 'update_available', latest: V2 } },
    ],
  });
  c.modsDatapackVersions.mockResolvedValue({ status: 'ok', data: [V2, V1] });
  c.modsProject.mockResolvedValue({
    status: 'ok',
    data: {
      summary: hit,
      description: '',
      website_url: null,
    },
  });
  c.runningInstances.mockResolvedValue([]);
  c.modsSearch.mockResolvedValue({
    status: 'ok',
    data: { hits: [], total: 0, offset: 0, page_size: 20 },
  });
  c.modsProjects.mockResolvedValue({ status: 'ok', data: [] });
  c.modsGetCurseforgeKeyStatus.mockResolvedValue({ status: 'ok', data: 'set' });
  c.modsListInstalled.mockResolvedValue({ status: 'ok', data: [] });
  c.assetsList.mockResolvedValue({ status: 'ok', data: [] });
});

afterEach(() => {
  for (const x of toastList()) dismiss(x.id);
  vi.clearAllMocks();
});

/** Check for updates, then press the pack's update button. */
async function updateFromLibrary(): Promise<void> {
  render(InstalledDatapacksView, { props: { instanceId: 'inst-1', mcVersion: '1.21.1' } });
  await fireEvent.click(await screen.findByRole('button', { name: /check for updates/i }));
  await fireEvent.click(await screen.findByTestId('datapack-update-btn'));
  await waitFor(() => expect(c.datapacksUpdateOne).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(warnings()).toHaveLength(1));
}

describe('InstalledDatapacksView — an update that left a world behind', () => {
  it('promises a retry, and keeps the badge for it, when the old copy was kept', async () => {
    c.datapacksUpdateOne.mockResolvedValue({ status: 'ok', data: outcome(true) });
    await updateFromLibrary();

    const [w] = warnings();
    expect(w.title).toMatch(/retry can finish/i);
    expect(w.lines).toEqual(['Alpha: locked']);
    expect(screen.getByTestId('datapack-update-btn')).toBeTruthy();
  });

  it('promises no retry, and drops the badge, when the library copy was replaced in place', async () => {
    c.datapacksUpdateOne.mockResolvedValue({ status: 'ok', data: outcome(false) });
    await updateFromLibrary();

    const [w] = warnings();
    expect(w.title).not.toMatch(/retry/i);
    expect(w.title).toMatch(/new version/i);
    expect(w.lines).toEqual(['Alpha: locked']);
    await waitFor(() => expect(screen.queryByTestId('datapack-update-btn')).toBeNull());
  });

  // «Update all» used to count this as failed and keep the badge. The badge
  // then offered a retry that could not reach Alpha: the second run finds
  // Alpha's copy foreign, skips it, and reports a clean update.
  it('Update all names the worlds and drops the badge when the library copy was replaced in place', async () => {
    c.datapacksUpdateOne.mockResolvedValue({ status: 'ok', data: outcome(false) });
    render(InstalledDatapacksView, { props: { instanceId: 'inst-1', mcVersion: '1.21.1' } });
    await fireEvent.click(await screen.findByRole('button', { name: /check for updates/i }));
    await fireEvent.click(await screen.findByRole('button', { name: /update all/i }));
    await waitFor(() => expect(c.datapacksUpdateOne).toHaveBeenCalledTimes(1));

    await waitFor(() => expect(warnings().some((w) => /new version/i.test(w.title))).toBe(true));
    const w = warnings().find((x) => /new version/i.test(x.title));
    expect(w?.title).not.toMatch(/retry/i);
    expect(w?.lines).toEqual(['Alpha: locked']);
    await waitFor(() => expect(screen.queryByTestId('datapack-update-btn')).toBeNull());
  });
});

describe('ModBrowseView — switching a library pack to another version', () => {
  async function switchInDrawer(): Promise<void> {
    c.modsSearch.mockResolvedValue({
      status: 'ok',
      data: { hits: [hit], total: 1, offset: 0, page_size: 20 },
    });
    render(ModBrowseView, {
      props: {
        source: 'modrinth',
        instanceId: 'inst-1',
        mcVersion: '1.21.1',
        loader: 'fabric',
        kind: 'datapack',
      },
    });
    await fireEvent.click(await screen.findByRole('button', { name: /^VeinMiner/ }));
    const modal = await screen.findByRole('dialog', { name: 'VeinMiner' });
    await fireEvent.click(within(modal).getByRole('tab', { name: 'Versions' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Switch to this version' }));
    await waitFor(() => expect(c.datapacksUpdateOne).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(warnings()).toHaveLength(1));
  }

  it('promises a retry when the old copy was kept', async () => {
    c.datapacksUpdateOne.mockResolvedValue({ status: 'ok', data: outcome(true) });
    await switchInDrawer();

    const [w] = warnings();
    expect(w.title).toMatch(/retry can finish/i);
    expect(w.lines).toEqual(['Alpha: locked']);
  });

  it('promises no retry when the library copy was replaced in place', async () => {
    c.datapacksUpdateOne.mockResolvedValue({ status: 'ok', data: outcome(false) });
    await switchInDrawer();

    const [w] = warnings();
    expect(w.title).not.toMatch(/retry/i);
    expect(w.title).toMatch(/new version/i);
    expect(w.lines).toEqual(['Alpha: locked']);
  });
});
