import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import ModBrowserTab from '$lib/mods/AddonsTab.svelte';

// Task 14 wires the real ModBrowseView in (Browse branch makes IPC
// calls on mount: mods_get_curseforge_key_status, mods_search). Task 17
// wires the real InstalledModsView in (Installed branch calls
// mods_list_installed on mount and subscribes to three events). Mock
// all of them so the test stays pure-DOM.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
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
    modsInstallLocal: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        sha1: 's',
        filename: 'a.jar',
        name: 'A',
        source: null,
        project_id: null,
        version_id: null,
        version_number: null,
        installed_at: '',
        enabled: true,
      },
    }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
  },
}));

// ModBrowserTab uses @tauri-apps/plugin-dialog for the "Install from file…"
// button. Stub it so happy-dom doesn't crash.
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue([]),
}));

import { commands } from '$lib/ipc/bindings';

describe('ModBrowserTab', () => {
  afterEach(async () => {
    const { droppedMods } = await import('$lib/settings/state.svelte');
    droppedMods.value = null;
  });

  it('defaults to the Browse view', () => {
    render(ModBrowserTab, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const browse = screen.getByRole('tab', { name: 'Browse' });
    const installed = screen.getByRole('tab', { name: 'Installed' });
    expect(browse.getAttribute('aria-selected')).toBe('true');
    expect(installed.getAttribute('aria-selected')).toBe('false');
    // Browse pane is mounted — its search input is the most stable
    // marker (label survives any visual reshuffle).
    expect(screen.getByLabelText('Search mods')).toBeTruthy();
  });

  it('switches to the Installed view on click', async () => {
    render(ModBrowserTab, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const installed = screen.getByRole('tab', { name: 'Installed' });
    await fireEvent.click(installed);
    expect(installed.getAttribute('aria-selected')).toBe('true');
    // The real InstalledModsView is now mounted (Task 17). Its filter
    // input is the most stable marker — label survives empty-state
    // and populated-list reshuffles alike.
    expect(screen.getByLabelText('Filter installed mods')).toBeTruthy();
  });

  it('accepts a null active instance (Mod browser opened with no instance selected)', () => {
    render(ModBrowserTab, {
      props: { instanceId: null, mcVersion: null, loader: null },
    });
    expect(screen.getByRole('tab', { name: 'Browse' })).toBeTruthy();
  });

  it('renders the FileDropzone affordance', () => {
    render(ModBrowserTab, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    expect(screen.getByTestId('file-dropzone')).toBeTruthy();
  });

  it('installs jars handed to it via the droppedMods rune', async () => {
    const { droppedMods } = await import('$lib/settings/state.svelte');
    render(ModBrowserTab, { props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' } });
    droppedMods.value = ['/x/a.jar'];
    await waitFor(() => {
      expect(vi.mocked(commands.modsInstallLocal)).toHaveBeenCalledWith('i', '/x/a.jar');
    });
    expect(droppedMods.value).toBeNull();
  });
});
