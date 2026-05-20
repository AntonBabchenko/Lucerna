import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import MainTabs from '$lib/layout/MainTabs.svelte';

// Task 14 made the Mod browser tab mount the real ModBrowseView, which
// fires modsGetCurseforgeKeyStatus + modsSearch on mount. Task 12 in
// sub-4 populated the Modpacks tab with the real ModpacksTab, which on
// activation runs modpackSearch (via ModpackBrowseView) and registers a
// webview drag-drop listener (via ImportDropzone). Stub all of them so
// the unrelated MainTabs assertions below don't trip on tauri-api
// errors from those background calls.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsSearch: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { hits: [], total: 0, offset: 0, page_size: 20 } }),
    modpackSearch: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { hits: [], total: 0, offset: 0, limit: 20 } }),
    modpackInspect: vi.fn(),
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
  },
  events: {},
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({ onDragDropEvent: () => Promise.resolve(() => {}) }),
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
