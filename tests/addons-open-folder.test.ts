// The Add-ons sub-tab row's folder button: one per content kind, on Browse and Installed alike,
// opening that kind's folder (data packs: the instance's library). Absent with no profile and on
// Mods for a Vanilla one, the instance menu's rule.

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { pushWarning, pushSuccess } = vi.hoisted(() => ({
  pushWarning: vi.fn(),
  pushSuccess: vi.fn(),
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({ pushWarning, pushSuccess }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    // The folder button under test.
    openAddonFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    // ModBrowseView (Browse branch) on mount
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsSearch: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { hits: [], total: 0, offset: 0, page_size: 20 } }),
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // ModDetailModal (opened by the Iris deep-link) loads a project + versions.
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsProject: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        summary: {
          source: 'modrinth',
          project_id: 'YL57xq9U',
          slug: 'iris',
          name: 'Iris Shaders',
          summary: 'A modern shaders mod for Minecraft.',
          icon_url: null,
          downloads: 0,
          author: 'coderbot',
          updated_at: null,
        },
        body_html: null,
        gallery: [],
      },
    }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: { roots: [] } }),
    instanceDependencyPreflight: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { violations: [] } }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsInspectLocal: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        loader_mismatch: false,
        platform_mismatch: false,
        platform_axis: null,
        platform_declared: null,
        detected_loader: null,
      },
    }),
    modsInstallLocal: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    // InstalledAssetsView (rp/shader installed list)
    assetsList: vi.fn().mockResolvedValue({ status: 'ok', data: { files: [], folders: [] } }),
    assetsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    assetInstall: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    assetUninstall: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    // Datapack kind: the 1.13+ gate + the library listing (Browse badges and
    // the Installed-datapacks view read the same command).
    instanceSupportsDatapacks: vi.fn().mockResolvedValue({ status: 'ok', data: true }),
    datapacksListLibrary: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { entries: [], worlds: [], library_folders: [] },
    }),
    datapacksCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    datapacksInstallFromFile: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    listWorldNames: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // InstalledDatapacksView (Data packs → Installed) reads the running state: it gates its
    // writes on the running game.
    runningInstances: vi.fn().mockResolvedValue([]),
  },
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

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn().mockResolvedValue([]) }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn().mockResolvedValue(undefined) }));

import { commands } from '$lib/ipc/bindings';
import AddonsTab from '$lib/mods/AddonsTab.svelte';
import { resetAddonsViewMemory } from '$lib/mods/addons-view-memory.svelte';
import { markSeen } from '$lib/onboarding/contextual-tours';

const props = {
  instanceId: 'i',
  instanceName: 'Test',
  mcVersion: '1.20.1',
  loader: 'fabric' as const,
  onManageLoader: () => {},
};

const folderButton = () => screen.queryByTestId('addons-open-folder');

describe('Add-ons folder button', () => {
  beforeEach(() => markSeen('addons'));
  afterEach(() => {
    resetAddonsViewMemory();
    vi.clearAllMocks();
  });

  it.each([
    ['Mods', 'Mods folder', 'mod'],
    ['Resource packs', 'Resource packs folder', 'resource_pack'],
    ['Shaders', 'Shaders folder', 'shader'],
    ['Data packs (Beta)', 'Data packs folder', 'datapack'],
  ])('on %s it reads «%s» and opens that kind', async (tab, label, kind) => {
    render(AddonsTab, { props });
    await fireEvent.click(screen.getByRole('tab', { name: tab }));
    const button = await screen.findByTestId('addons-open-folder');
    expect(button.textContent?.trim()).toBe(label);
    await fireEvent.click(button);
    expect(commands.openAddonFolder).toHaveBeenCalledWith('i', kind);
  });

  it('is there on Browse and on Installed', async () => {
    render(AddonsTab, { props });
    await fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));
    expect(folderButton()).toBeTruthy();
    await fireEvent.click(screen.getByRole('tab', { name: 'Installed' }));
    expect(folderButton()).toBeTruthy();
  });

  it('is absent with no profile', () => {
    render(AddonsTab, { props: { ...props, instanceId: null } });
    expect(folderButton()).toBeNull();
  });

  it('is absent on Mods for a Vanilla profile, and there on its other kinds', async () => {
    render(AddonsTab, { props: { ...props, loader: 'vanilla' as const } });
    expect(folderButton()).toBeNull();
    await fireEvent.click(screen.getByRole('tab', { name: 'Shaders' }));
    expect(folderButton()).toBeTruthy();
  });

  it('a folder that cannot be opened says so', async () => {
    vi.mocked(commands.openAddonFolder).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: 'x', details: 'denied' },
    } as never);
    render(AddonsTab, { props });
    await fireEvent.click(await screen.findByTestId('addons-open-folder'));
    await waitFor(() =>
      expect(pushWarning).toHaveBeenCalledWith("Couldn't open the folder.", expect.any(Array)),
    );
  });
});
