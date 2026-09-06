import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstanceWithStatus } from '$lib/ipc/bindings';
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
    // The world-list commands that used to be stubbed here are gone with the
    // real WorldsTab: it is replaced by the prop-capturing stub below, so
    // nothing in this file reaches them any more.
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

// What the real WorldsTab does with the props is its own file's business
// (tests/worlds-tab-migrate.test.ts); MainTabs' share of the post-migration
// chain is that it forwards them at all. Svelte 5 calls a component as
// `(anchor, props)`, so the stub renders nothing and keeps what it was handed
// — the same trick as tests/worlds-tab-migrate.test.ts uses for the migrate
// dialog. The tab-label and active-mirror assertions below never look inside
// the Worlds panel, so stubbing it costs them nothing.
const worldsTab = vi.hoisted(() => ({ props: null as Record<string, unknown> | null }));
vi.mock('$lib/worlds/WorldsTab.svelte', () => ({
  default: function stubWorldsTab(_anchor: unknown, props: Record<string, unknown>) {
    worldsTab.props = props;
    return {};
  },
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

// The Worlds tab's migrate dialog needs a target list it cannot fetch, and its
// landed outcome has to reach the Play menu, which lives above MainTabs. Both
// travel as props through here — one-line forwards that nothing else pins
// (world-migration spec §7 "Completion", §9 "Frontend").
function instance(over: Partial<InstanceWithStatus>): InstanceWithStatus {
  return {
    id: 'src',
    name: 'Source',
    mc_version: '1.21.1',
    loader: 'fabric',
    loader_version: '0.16.0',
    max_heap_mb: 4096,
    min_heap_mb: null,
    extra_jvm_args: '',
    created_unix_ms: null,
    ready: true,
    has_icon: false,
    mrpack_name: null,
    mrpack_version: null,
    mrpack_project_id: null,
    mrpack_source: null,
    mrpack_summary: null,
    mrpack_version_id: null,
    integrity: null,
    imported_from: null,
    created_from_server: null,
    ...over,
  };
}

describe('MainTabs — what the Worlds tab is handed', () => {
  afterEach(() => {
    worldsTab.props = null;
  });

  it('forwards the instance list and the worlds-changed callback it was given', async () => {
    const onWorldsChanged = vi.fn();
    render(MainTabs, {
      props: {
        instanceId: 'src',
        instanceName: 'Source',
        instances: [
          instance({ id: 'src', name: 'Source' }),
          instance({ id: 'dst', name: 'Target' }),
        ],
        onWorldsChanged,
      },
    });
    await fireEvent.click(screen.getByRole('tab', { name: 'Worlds' }));
    await waitFor(() => expect(worldsTab.props).not.toBeNull());

    // By content, not identity: the props reach the child through Svelte's
    // reactive plumbing, which is free to hand over a different reference.
    const forwarded = worldsTab.props?.instances as InstanceWithStatus[];
    expect(forwarded.map((i) => i.id)).toEqual(['src', 'dst']);
    expect(worldsTab.props?.instanceName).toBe('Source');

    // The callback must be the parent's own, not a swallowing default — the
    // Play menu refresh is the whole point of the forward.
    (worldsTab.props?.onWorldsChanged as () => void)();
    expect(onWorldsChanged).toHaveBeenCalledTimes(1);
  });
});
