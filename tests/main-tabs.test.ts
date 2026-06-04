import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { commands } from '$lib/ipc/bindings';
import MainTabs from '$lib/layout/MainTabs.svelte';

// Mod browser mounts ModBrowseView on activation, which fires
// modsGetCurseforgeKeyStatus + modsSearch on mount. Stub them so the
// unrelated MainTabs assertions don't trip on tauri-api errors.
// Modpacks live at the sidebar level now (not as a tab), so their
// command stubs moved to tests/sidebar.test.ts.
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
        detected_loader: null,
        detected_mc: null,
        detected_name: null,
        loader_mismatch: false,
        mc_mismatch: false,
      },
    }),
    modsInstallLocal: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    listWorlds: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    processExited: { listen: () => Promise.resolve(() => {}) },
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
    expect(getByText('Add-ons')).toBeTruthy();
    expect(getByText('Worlds')).toBeTruthy();
  });

  it('does not render a Modpacks tab (moved to sidebar)', () => {
    const { queryByText } = render(MainTabs, { props: {} });
    expect(queryByText('Modpacks')).toBeNull();
  });

  it('starts on Overview tab', () => {
    const { getByText } = render(MainTabs, { props: {} });
    const overview = getByText('Overview').closest('button');
    expect(overview?.getAttribute('aria-selected')).toBe('true');
  });

  it('switches active tab on click', async () => {
    const { getByText } = render(MainTabs, { props: {} });
    await fireEvent.click(getByText('Add-ons'));
    const browser = getByText('Add-ons').closest('button');
    expect(browser?.getAttribute('aria-selected')).toBe('true');
  });

  it('renders ModBrowserTab when Mod browser tab is active', async () => {
    const { getByText, getByLabelText } = render(MainTabs, { props: {} });
    await fireEvent.click(getByText('Add-ons'));
    expect(getByText('Browse')).toBeTruthy();
    expect(getByText('Installed')).toBeTruthy();
    expect(getByLabelText('Mod source')).toBeTruthy();
  });

  it('Mod browser tab carries data-tour attribute', () => {
    const { getByText } = render(MainTabs, { props: {} });
    const mods = getByText('Add-ons').closest('button');
    expect(mods?.getAttribute('data-tour')).toBe('tab-mods');
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
    await fireEvent.click(screen.getByRole('tab', { name: 'Add-ons' }));
    await flushMount();
    dragDropHandlers.fire({ payload: { type: 'drop', paths: ['/x/a.jar', '/x/readme.txt'] } });
    // ModBrowserTab immediately consumes droppedMods and triggers the install
    // flow, so by the time we check, the rune is already reset to null.
    await waitFor(() => {
      expect(vi.mocked(commands.modsInstallLocal)).toHaveBeenCalledWith('i', '/x/a.jar');
    });
    const { droppedModpack } = await import('$lib/settings/state.svelte');
    expect(droppedModpack.value).toBeNull();
  });

  it('ignores .mrpack/.zip drops in MainTabs (modpacks moved to sidebar)', async () => {
    render(MainTabs, { props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' } });
    await flushMount();
    await fireEvent.click(screen.getByRole('tab', { name: 'Add-ons' }));
    await flushMount();
    dragDropHandlers.fire({ payload: { type: 'drop', paths: ['/x/pack.mrpack'] } });
    const { droppedModpack } = await import('$lib/settings/state.svelte');
    expect(droppedModpack.value).toBeNull();
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
    await fireEvent.click(screen.getByRole('tab', { name: 'Add-ons' }));
    await flushMount();
    dragDropHandlers.fire({ payload: { type: 'drop', paths: ['/x/a.jar'] } });
    const { droppedMods } = await import('$lib/settings/state.svelte');
    expect(droppedMods.value).toBeNull();
  });

  it('flips the dragActive rune on drag enter over the Mods tab and back on leave', async () => {
    render(MainTabs, { props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' } });
    await flushMount();
    await fireEvent.click(screen.getByRole('tab', { name: 'Add-ons' }));
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
