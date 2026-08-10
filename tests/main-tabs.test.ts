import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import MainTabs from '$lib/layout/MainTabs.svelte';
import { markSeen } from '$lib/onboarding/contextual-tours';

// Mod browser mounts ModBrowseView on activation, which fires
// modsGetCurseforgeKeyStatus + modsSearch on mount. Stub them so the
// unrelated MainTabs assertions don't trip on tauri-api errors.
// Modpacks live at the sidebar level now (not as a tab), so their
// command stubs moved to tests/sidebar.test.ts.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsSearch: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { hits: [], total: 0, offset: 0, page_size: 20 } }),
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: { roots: [] } }),
    instanceDependencyPreflight: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { violations: [] } }),
    modsInspectLocal: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        detected_loader: null,
        detected_name: null,
        loader_mismatch: false,
        platform_mismatch: false,
        platform_axis: null,
        platform_declared: null,
      },
    }),
    modsInstallLocal: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    assetInstallLocal: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    assetsList: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listWorlds: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    processExited: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({
  Channel: vi.fn(),
}));

beforeEach(() => markSeen('addons'));

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

// The window-level drag-drop listener moved to +page.svelte; its routing
// matrix is covered by tests/drop-router.test.ts. MainTabs' remaining part
// of that contract is the active-tab mirror the router reads.
describe('MainTabs active-tab mirror', () => {
  afterEach(async () => {
    const s = await import('$lib/settings/state.svelte');
    s.clientActiveTab.value = 'overview';
  });

  it('publishes the active tab to clientActiveTab for the window drop router', async () => {
    render(MainTabs, { props: {} });
    const { clientActiveTab } = await import('$lib/settings/state.svelte');
    expect(clientActiveTab.value).toBe('overview');
    await fireEvent.click(screen.getByRole('tab', { name: 'Worlds' }));
    expect(clientActiveTab.value).toBe('worlds');
  });
});
