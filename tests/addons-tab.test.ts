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
  });

  it('renders the content-kind switch with the three labels and the mod dropzone by default', () => {
    render(AddonsTab, { props });
    expect(screen.getByRole('radio', { name: 'Mods' })).toBeTruthy();
    expect(screen.getByRole('radio', { name: 'Resource packs' })).toBeTruthy();
    expect(screen.getByRole('radio', { name: 'Shaders' })).toBeTruthy();
    // Default kind is 'mod' → the .jar dropzone affordance is present.
    expect(screen.getByTestId('file-dropzone')).toBeTruthy();
    // Mods radio is selected by default.
    expect(screen.getByRole('radio', { name: 'Mods' }).getAttribute('aria-checked')).toBe('true');
  });

  it('shows the shader hint banner and hides the dropzone when Shaders is selected', async () => {
    render(AddonsTab, { props });
    await fireEvent.click(screen.getByRole('radio', { name: 'Shaders' }));
    await waitFor(() => {
      expect(screen.getByText('Shaders need Iris or OptiFine installed to run.')).toBeTruthy();
    });
    expect(screen.queryByTestId('file-dropzone')).toBeNull();
  });

  it('shows no hint banner and no dropzone when Resource packs is selected', async () => {
    render(AddonsTab, { props });
    await fireEvent.click(screen.getByRole('radio', { name: 'Resource packs' }));
    await waitFor(() => {
      expect(
        screen.getByRole('radio', { name: 'Resource packs' }).getAttribute('aria-checked'),
      ).toBe('true');
    });
    expect(screen.queryByText('Shaders need Iris or OptiFine installed to run.')).toBeNull();
    expect(screen.queryByTestId('file-dropzone')).toBeNull();
  });
});
