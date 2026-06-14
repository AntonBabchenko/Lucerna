// AddonsTab behaviour: the content-kind switch (Mods · Resource packs ·
// Shaders) and how the mod-only chrome (the .jar dropzone) and the shader
// hint banner appear/disappear by kind.
//
// The tab pulls in ModBrowseView (fires IPC on mount), InstalledModsView,
// and InstalledAssetsView. We mock the whole bindings layer so the tab
// renders against empty results and no real Tauri calls happen.

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    // ModBrowseView (Browse branch) on mount
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsSearch: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { hits: [], total: 0, offset: 0, page_size: 20 } }),
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // ModDetailModal (opened by the Iris deep-link) loads a project + versions.
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
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsInspectLocal: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        loader_mismatch: false,
        mc_mismatch: false,
        detected_loader: null,
        detected_mc: null,
      },
    }),
    modsInstallLocal: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    // InstalledAssetsView (rp/shader installed list)
    assetsList: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    assetsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    assetInstall: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    assetUninstall: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn().mockResolvedValue([]) }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn().mockResolvedValue(undefined) }));

import { commands } from '$lib/ipc/bindings';
import AddonsTab from '$lib/mods/AddonsTab.svelte';

const props = {
  instanceId: 'i',
  instanceName: 'Test',
  mcVersion: '1.20.1',
  loader: 'fabric' as const,
};

describe('AddonsTab', () => {
  afterEach(async () => {
    const { droppedMods, modBrowseOpenProject } = await import('$lib/settings/state.svelte');
    droppedMods.value = null;
    modBrowseOpenProject.value = null;
    // Reset all mock call counts between tests so assertions about "not called"
    // are not poisoned by invocations from earlier tests.
    vi.clearAllMocks();
  });

  it('renders the content-kind switch with the three labels and the mod dropzone by default', () => {
    render(AddonsTab, { props });
    expect(screen.getByRole('tab', { name: 'Mods' })).toBeTruthy();
    expect(screen.getByRole('tab', { name: 'Resource packs' })).toBeTruthy();
    expect(screen.getByRole('tab', { name: 'Shaders' })).toBeTruthy();
    // Default kind is 'mod' → the .jar dropzone affordance is present.
    expect(screen.getByTestId('file-dropzone')).toBeTruthy();
    // Mods radio is selected by default.
    expect(screen.getByRole('tab', { name: 'Mods' }).getAttribute('aria-selected')).toBe('true');
  });

  it('on Fabric shows only Iris (no OptiFine, no Oculus) plus the shader dropzone', async () => {
    render(AddonsTab, { props }); // props.loader === 'fabric'
    await fireEvent.click(screen.getByRole('tab', { name: 'Shaders' }));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Iris' })).toBeTruthy();
    });
    expect(screen.queryByRole('button', { name: /OptiFine/ })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Oculus' })).toBeNull();
    expect(screen.getByTestId('file-dropzone')).toBeTruthy();
  });

  it('on vanilla the hint offers OptiFine and names the instance MC version', async () => {
    render(AddonsTab, { props: { ...props, loader: 'vanilla' } });
    await fireEvent.click(screen.getByRole('tab', { name: 'Shaders' }));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /OptiFine/ })).toBeTruthy();
    });
    expect(screen.queryByRole('button', { name: 'Iris' })).toBeNull();
    expect(screen.getByText(/Minecraft 1\.20\.1/)).toBeTruthy();
  });

  it('with no instance offers all three loaders and asks to pick an instance', async () => {
    render(AddonsTab, { props: { ...props, instanceId: null, mcVersion: null, loader: null } });
    await fireEvent.click(screen.getByRole('tab', { name: 'Shaders' }));
    await waitFor(() => {
      expect(screen.getByText(/select an instance to see which build/i)).toBeTruthy();
    });
    expect(screen.getByRole('button', { name: 'Iris' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Oculus' })).toBeTruthy();
    expect(screen.getByRole('button', { name: /OptiFine/ })).toBeTruthy();
  });

  it('clicking Iris jumps to the Mods segment and opens the Iris detail modal', async () => {
    render(AddonsTab, { props });
    await fireEvent.click(screen.getByRole('tab', { name: 'Shaders' }));
    const iris = await screen.findByRole('button', { name: 'Iris' });
    await fireEvent.click(iris);

    // Mods segment becomes active (kind flipped to 'mod').
    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Mods' }).getAttribute('aria-selected')).toBe('true');
    });
    // The detail modal opened against the Iris project.
    expect(await screen.findByRole('dialog')).toBeTruthy();
    expect(await screen.findByText('Iris Shaders')).toBeTruthy();
  });

  it('clicking OptiFine opens the optifine.net downloads page externally', async () => {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    render(AddonsTab, { props: { ...props, loader: 'vanilla' } });
    await fireEvent.click(screen.getByRole('tab', { name: 'Shaders' }));
    const optifine = await screen.findByRole('button', { name: /OptiFine/ });
    await fireEvent.click(optifine);
    await waitFor(() => {
      expect(openUrl).toHaveBeenCalledWith('https://optifine.net/downloads');
    });
  });

  it('shows no hint banner but shows the resource-pack dropzone when Resource packs is selected', async () => {
    render(AddonsTab, { props });
    await fireEvent.click(screen.getByRole('tab', { name: 'Resource packs' }));
    await waitFor(() => {
      expect(
        screen.getByRole('tab', { name: 'Resource packs' }).getAttribute('aria-selected'),
      ).toBe('true');
    });
    expect(screen.queryByRole('button', { name: 'Iris' })).toBeNull();
    expect(screen.getByTestId('file-dropzone')).toBeTruthy();
  });

  it('switching kind resets to Browse sub-view', async () => {
    render(AddonsTab, { props });

    // Open the Installed sub-tab while on Mods.
    await fireEvent.click(screen.getByRole('tab', { name: 'Installed' }));
    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Installed' }).getAttribute('aria-selected')).toBe(
        'true',
      );
    });

    // Switch to Shaders — the kind-reset effect must land on Browse.
    await fireEvent.click(screen.getByRole('tab', { name: 'Shaders' }));
    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Browse' }).getAttribute('aria-selected')).toBe(
        'true',
      );
    });
    expect(screen.getByRole('tab', { name: 'Installed' }).getAttribute('aria-selected')).toBe(
      'false',
    );

    // InstalledAssetsView must NOT be mounted — installedMounted was reset to
    // false by the kind switch. We assert on its unique "Check for updates"
    // button rather than assetsList: the Browse pane (ModBrowseView) now also
    // calls assetsList on mount for non-mod kinds to drive installed-state
    // badges, so that command is no longer a mount signal for the Installed view.
    expect(screen.queryByRole('button', { name: 'Check for updates' })).toBeNull();
  });

  it('switching kind does not auto-mount Installed sub-view (no premature IPC)', async () => {
    render(AddonsTab, { props });

    // Switch through all non-mod kinds without opening the Installed tab.
    await fireEvent.click(screen.getByRole('tab', { name: 'Resource packs' }));
    await fireEvent.click(screen.getByRole('tab', { name: 'Shaders' }));

    // Browse is still the active sub-tab.
    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Browse' }).getAttribute('aria-selected')).toBe(
        'true',
      );
    });

    // InstalledAssetsView was never mounted — assert on its unique "Check for
    // updates" button. (assetsList is no longer a valid mount signal: the Browse
    // pane now calls it on mount for non-mod kinds to render installed badges.)
    expect(screen.queryByRole('button', { name: 'Check for updates' })).toBeNull();
  });

  it('both tablists (content-kind switch + sub-tabs) have an accessible name', () => {
    render(AddonsTab, { props });
    const tablists = screen.getAllByRole('tablist');
    expect(tablists.length).toBeGreaterThanOrEqual(2);
    for (const tl of tablists) {
      expect(tl.getAttribute('aria-label')).toBeTruthy();
    }
  });

  it('on Forge offers Oculus and OptiFine but not Iris', async () => {
    render(AddonsTab, { props: { ...props, loader: 'forge' } });
    await fireEvent.click(screen.getByRole('tab', { name: 'Shaders' }));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Oculus' })).toBeTruthy();
    });
    expect(screen.getByRole('button', { name: /OptiFine/ })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Iris' })).toBeNull();
  });

  it('on NeoForge offers Iris and Oculus but not OptiFine', async () => {
    render(AddonsTab, { props: { ...props, loader: 'neoforge' } });
    await fireEvent.click(screen.getByRole('tab', { name: 'Shaders' }));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Iris' })).toBeTruthy();
    });
    expect(screen.getByRole('button', { name: 'Oculus' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /OptiFine/ })).toBeNull();
  });

  it('clicking Oculus jumps to the Mods segment and deep-links the Oculus project', async () => {
    render(AddonsTab, { props: { ...props, loader: 'forge' } });
    await fireEvent.click(screen.getByRole('tab', { name: 'Shaders' }));
    const oculus = await screen.findByRole('button', { name: 'Oculus' });
    // openOculus deep-links the Oculus Modrinth project (GchcoXML) into the
    // freshly-mounted mod browser, which consumes modBrowseOpenProject on mount.
    await fireEvent.click(oculus);
    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Mods' }).getAttribute('aria-selected')).toBe('true');
    });
    // The mod browser consumed the deep-link and opened the detail modal — this
    // proves openOculus set modBrowseOpenProject (the rune is cleared on consume,
    // so we assert on the resulting modal, mirroring the Iris test).
    const { commands: c } = await import('$lib/ipc/bindings');
    await waitFor(() => {
      expect(vi.mocked(c.modsProject)).toHaveBeenCalledWith('modrinth', 'GchcoXML');
    });
  });

  it('hides the hint entirely when an applicable shader loader is already installed', async () => {
    // Iris already installed on a Fabric instance → no banner.
    vi.mocked(commands.modsListInstalled).mockResolvedValue({
      status: 'ok',
      data: [
        {
          filename: 'iris-mc1.20.1-1.7.0.jar',
          sha1: 'a',
          source: 'modrinth',
          project_id: 'YL57xq9U',
          version_id: null,
          name: 'Iris',
          version_number: null,
          installed_at: '2026-06-14T00:00:00Z',
          enabled: true,
          requires: [],
          enrich_attempted: false,
        },
      ],
    });
    render(AddonsTab, { props }); // fabric
    await fireEvent.click(screen.getByRole('tab', { name: 'Shaders' }));
    // The banner shows momentarily (installed list starts empty) then hides once
    // detection resolves — wait for the Iris button to disappear.
    await waitFor(() => {
      expect(screen.queryByRole('button', { name: 'Iris' })).toBeNull();
    });
    // Restore the factory default so later tests see an empty installed list.
    vi.mocked(commands.modsListInstalled).mockResolvedValue({ status: 'ok', data: [] });
  });
});
