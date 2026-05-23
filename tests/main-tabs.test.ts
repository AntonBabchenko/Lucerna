import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { commands } from '$lib/ipc/bindings';
import MainTabs from '$lib/layout/MainTabs.svelte';

// Task 14 made the Mod browser tab mount the real ModBrowseView, which
// fires modsGetCurseforgeKeyStatus + modsSearch on mount. Task 12 in
// sub-4 populated the Modpacks tab with the real ModpacksTab, which on
// activation runs modpackSearch (via ModpackBrowseView). Stub all of
// them so the unrelated MainTabs assertions below don't trip on
// tauri-api errors from those background calls.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsSearch: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { hits: [], total: 0, offset: 0, page_size: 20 } }),
    modpackSearch: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { hits: [], total: 0, offset: 0, limit: 20 } }),
    modpackInspect: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        format: 'modrinth',
        name: 'Test Pack',
        version: '1.0',
        game_version: '1.20.1',
        loader: 'fabric',
        loader_version: null,
        files: [],
        unresolvable: [],
        has_overrides: false,
        has_client_overrides: false,
        has_saves_in_overrides: false,
      },
    }),
    modpackImport: vi.fn(),
    modpackFetchToTemp: vi.fn(),
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // Bundle 2: ImportedView fetches modpack_status per pack on mount;
    // ImportedDetailDrawer additionally hits modsProject and
    // modpackRestoreFile. Both ImportedView (`/Imported` sub-tab) and
    // the drawer (card click) can be mounted by the main-tabs flow, so
    // we need stub returns for all of them.
    modpackStatus: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsProject: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modpackRestoreFile: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    deleteInstance: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modpackCheckUpdate: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsInspectLocal: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        detected_loader: null,
        detected_mc: null,
        detected_name: null,
        loader_mismatch: false,
        mc_mismatch: false,
      },
    }),
    modsInstallLocal: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
  },
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
// MainTabs registers one window-level drag-drop listener. The mock stores
// the registered callback so tests can simulate OS drag-drop events.
const dragDropHandlers = vi.hoisted(() => ({
  cbs: [] as Array<(e: unknown) => void>,
  fire(e: unknown) {
    for (const cb of dragDropHandlers.cbs) cb(e);
  },
}));
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (cb: (e: unknown) => void) => {
      dragDropHandlers.cbs.push(cb);
      return Promise.resolve(() => {
        const idx = dragDropHandlers.cbs.indexOf(cb);
        if (idx !== -1) dragDropHandlers.cbs.splice(idx, 1);
      });
    },
  }),
}));
vi.mock('@tauri-apps/api/core', () => ({
  Channel: vi.fn(),
}));

describe('MainTabs', () => {
  it('renders the three tab labels', () => {
    const { getByText } = render(MainTabs, { props: {} });
    expect(getByText('Overview')).toBeTruthy();
    expect(getByText('Mod browser')).toBeTruthy();
    expect(getByText('Modpacks')).toBeTruthy();
  });

  it('starts on Overview tab', () => {
    const { getByText } = render(MainTabs, { props: {} });
    const overview = getByText('Overview').closest('button');
    expect(overview?.getAttribute('aria-selected')).toBe('true');
  });

  it('switches active tab on click', async () => {
    const { getByText } = render(MainTabs, { props: {} });
    await fireEvent.click(getByText('Mod browser'));
    const browser = getByText('Mod browser').closest('button');
    expect(browser?.getAttribute('aria-selected')).toBe('true');
  });

  it('renders ModBrowserTab when Mod browser tab is active', async () => {
    const { getByText, getByLabelText } = render(MainTabs, { props: {} });
    await fireEvent.click(getByText('Mod browser'));
    // ModBrowserTab renders the Browse / Installed sub-tabs and the
    // Source picker — assert on those rather than placeholder text.
    expect(getByText('Browse')).toBeTruthy();
    expect(getByText('Installed')).toBeTruthy();
    expect(getByLabelText('Mod source')).toBeTruthy();
  });

  it('renders ModpacksTab when Modpacks tab is active', async () => {
    const { getByText, getAllByRole } = render(MainTabs, { props: {} });
    await fireEvent.click(getByText('Modpacks'));
    // ModpacksTab renders its own Browse | Imported sub-tabs.
    const tabs = getAllByRole('tab').map((t) => t.textContent?.trim());
    expect(tabs).toContain('Browse');
    expect(tabs).toContain('Imported');
  });

  it('switches back to Overview when Open instance fires from Imported drawer', async () => {
    const importedPack = {
      id: 'pack-1',
      name: 'Pack instance',
      mc_version: '1.20.1',
      loader: 'fabric' as const,
      loader_version: '0.15.7',
      max_heap_mb: 2048,
      extra_jvm_args: '',
      created_unix_ms: Date.now(),
      ready: true,
      mrpack_name: 'Test Pack',
      mrpack_version: '1.0',
      mrpack_project_id: null,
      mrpack_source: null,
      mrpack_summary: null,
      mrpack_version_id: null,
    };
    const onSwitchInstance = vi.fn();
    const { getByText, getByTestId, getAllByTestId } = render(MainTabs, {
      props: { instances: [importedPack], onSwitchInstance },
    });
    // Navigate: top-level Modpacks → Imported sub-tab → card → Open instance.
    await fireEvent.click(getByText('Modpacks'));
    await fireEvent.click(getByText('Imported'));
    const cards = getAllByTestId('imported-card');
    await fireEvent.click(cards[0]);
    await fireEvent.click(getByTestId('imported-detail-open'));
    // Active tab is back on Overview.
    const overviewTab = getByText('Overview').closest('button');
    expect(overviewTab?.getAttribute('aria-selected')).toBe('true');
    expect(onSwitchInstance).toHaveBeenCalledWith('pack-1');
  });

  it('Mod browser and Modpacks tab buttons carry data-tour attributes', () => {
    const { getByText } = render(MainTabs, { props: {} });
    const mods = getByText('Mod browser').closest('button');
    const modpacks = getByText('Modpacks').closest('button');
    expect(mods?.getAttribute('data-tour')).toBe('tab-mods');
    expect(modpacks?.getAttribute('data-tour')).toBe('tab-modpacks');
  });
});

// Allow onMount + listener registration to complete before firing events.
async function flushMount() {
  await tick();
  await new Promise((r) => setTimeout(r, 0));
  await tick();
}

describe('MainTabs drag-drop routing', () => {
  afterEach(async () => {
    const s = await import('$lib/settings/state.svelte');
    s.droppedMods.value = null;
    s.droppedModpack.value = null;
    s.dragActive.value = false;
    dragDropHandlers.cbs.length = 0;
  });

  it('routes a .jar drop on the Mods tab to droppedMods', async () => {
    render(MainTabs, { props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' } });
    await flushMount();
    await fireEvent.click(screen.getByRole('tab', { name: 'Mod browser' }));
    await flushMount();
    dragDropHandlers.fire({ payload: { type: 'drop', paths: ['/x/a.jar', '/x/readme.txt'] } });
    // ModBrowserTab immediately consumes droppedMods and triggers the install
    // flow, so by the time we check, the rune is already reset to null and
    // modsInstallLocal has been (or is being) called.
    await waitFor(() => {
      expect(vi.mocked(commands.modsInstallLocal)).toHaveBeenCalledWith('i', '/x/a.jar');
    });
    const { droppedModpack } = await import('$lib/settings/state.svelte');
    expect(droppedModpack.value).toBeNull();
  });

  it('routes a .mrpack drop on the Modpacks tab to droppedModpack', async () => {
    render(MainTabs, { props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' } });
    await flushMount();
    await fireEvent.click(screen.getByRole('tab', { name: 'Modpacks' }));
    await flushMount();
    dragDropHandlers.fire({ payload: { type: 'drop', paths: ['/x/pack.mrpack'] } });
    // ModpacksTab immediately consumes droppedModpack and triggers
    // modpackInspect, so by the time we check, the rune is already reset
    // to null — same pattern as the .jar / modsInstallLocal test above.
    await waitFor(() => {
      expect(vi.mocked(commands.modpackInspect)).toHaveBeenCalledWith('/x/pack.mrpack');
    });
    const { droppedMods, droppedModpack } = await import('$lib/settings/state.svelte');
    expect(droppedModpack.value).toBeNull();
    expect(droppedMods.value).toBeNull();
  });

  it('routes a .zip drop on the Modpacks tab to droppedModpack', async () => {
    render(MainTabs, { props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' } });
    await flushMount();
    await fireEvent.click(screen.getByRole('tab', { name: 'Modpacks' }));
    await flushMount();
    dragDropHandlers.fire({ payload: { type: 'drop', paths: ['/x/curse-pack.zip'] } });
    await waitFor(() => {
      expect(vi.mocked(commands.modpackInspect)).toHaveBeenCalledWith('/x/curse-pack.zip');
    });
  });

  it('ignores a drop on the Overview tab', async () => {
    render(MainTabs, { props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' } });
    await flushMount();
    dragDropHandlers.fire({ payload: { type: 'drop', paths: ['/x/a.jar'] } });
    const { droppedMods, droppedModpack } = await import('$lib/settings/state.svelte');
    expect(droppedMods.value).toBeNull();
    expect(droppedModpack.value).toBeNull();
  });

  it('does not route a .jar when there is no installable instance', async () => {
    render(MainTabs, { props: { instanceId: null, mcVersion: null, loader: null } });
    await flushMount();
    await fireEvent.click(screen.getByRole('tab', { name: 'Mod browser' }));
    await flushMount();
    dragDropHandlers.fire({ payload: { type: 'drop', paths: ['/x/a.jar'] } });
    const { droppedMods } = await import('$lib/settings/state.svelte');
    expect(droppedMods.value).toBeNull();
  });

  it('flips the dragActive rune on drag enter over the Mods tab and back on leave', async () => {
    render(MainTabs, { props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' } });
    await flushMount();
    await fireEvent.click(screen.getByRole('tab', { name: 'Mod browser' }));
    await flushMount();
    const { dragActive } = await import('$lib/settings/state.svelte');
    expect(dragActive.value).toBe(false);
    dragDropHandlers.fire({ payload: { type: 'enter' } });
    expect(dragActive.value).toBe(true);
    dragDropHandlers.fire({ payload: { type: 'leave' } });
    expect(dragActive.value).toBe(false);
  });

  it('leaves dragActive false on a drag over the Overview tab', async () => {
    render(MainTabs, { props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' } });
    await flushMount();
    dragDropHandlers.fire({ payload: { type: 'enter' } });
    const { dragActive } = await import('$lib/settings/state.svelte');
    expect(dragActive.value).toBe(false);
  });
});
