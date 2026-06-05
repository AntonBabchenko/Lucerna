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
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: { roots: [] } }),
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
  },
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn().mockResolvedValue([]) }));

import AddonsTab from '$lib/mods/AddonsTab.svelte';

const props = {
  instanceId: 'i',
  instanceName: 'Test',
  mcVersion: '1.20.1',
  loader: 'fabric' as const,
};

describe('AddonsTab', () => {
  afterEach(async () => {
    const { droppedMods } = await import('$lib/settings/state.svelte');
    droppedMods.value = null;
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

  it('shows the shader hint banner and hides the dropzone when Shaders is selected', async () => {
    render(AddonsTab, { props });
    await fireEvent.click(screen.getByRole('tab', { name: 'Shaders' }));
    await waitFor(() => {
      expect(screen.getByText('Shaders need Iris or OptiFine installed to run.')).toBeTruthy();
    });
    expect(screen.queryByTestId('file-dropzone')).toBeNull();
  });

  it('shows no hint banner and no dropzone when Resource packs is selected', async () => {
    render(AddonsTab, { props });
    await fireEvent.click(screen.getByRole('tab', { name: 'Resource packs' }));
    await waitFor(() => {
      expect(
        screen.getByRole('tab', { name: 'Resource packs' }).getAttribute('aria-selected'),
      ).toBe('true');
    });
    expect(screen.queryByText('Shaders need Iris or OptiFine installed to run.')).toBeNull();
    expect(screen.queryByTestId('file-dropzone')).toBeNull();
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
});
