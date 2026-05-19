import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import ModBrowserTab from '$lib/mods/ModBrowserTab.svelte';

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
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
  },
}));

describe('ModBrowserTab', () => {
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
});
