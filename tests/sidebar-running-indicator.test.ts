import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { SvelteMap } from 'svelte/reactivity';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Account, InstanceWithStatus } from '$lib/ipc/bindings';
import Sidebar from '$lib/layout/Sidebar.svelte';
import { initSidebarButtons } from '$lib/layout/sidebar-buttons.svelte';

// Same mount seam the other Sidebar tests use: stub the IPC bindings + the
// tauri dialog/core plugins so the component (and its children) can mount
// without a real Tauri host.
vi.mock('$lib/ipc/bindings', () => ({
  commands: { accountSkin: vi.fn().mockResolvedValue({ status: 'ok', data: null }) },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    processExited: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn().mockResolvedValue(null) }));
vi.mock('@tauri-apps/api/core', () => ({ Channel: vi.fn() }));

function offlineAccount(over: Partial<Account> = {}): Account {
  return {
    id: 'of-1',
    kind: 'offline',
    name: 'Steve',
    uuid: '00000000-0000-0000-0000-000000000001',
    expires_at: null,
    ...over,
  };
}

function instance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-1',
    name: 'Default',
    mc_version: '1.20.4',
    loader: 'vanilla',
    loader_version: null,
    max_heap_mb: 2048,
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

const noopHandlers = {
  onSelectAccount: () => {},
  onRemoveAccount: () => {},
  onOpenCosmetics: () => {},
  onAddOffline: () => {},
  onSelectInstance: () => {},
  onOpenManage: () => {},
  onManageInstance: () => {},
  onOpenMods: () => {},
  onOpenLogs: () => {},
  onOpenModpacks: () => {},
  onOpenLauncherImport: () => {},
  onOpenGallery: () => {},
  onOpenQuickJoin: () => {},
  onPlay: () => {},
  onStop: () => {},
  onInstall: () => {},
};

// active = inst-1 (idle); inst-2 is the OTHER, running instance. Keeping the
// running instance non-active means the closed trigger (which shows the active
// one) has no badge, so an open-dropdown badge query is unambiguous.
const instA = instance({ id: 'inst-1', name: 'Alpha' });
const instB = instance({ id: 'inst-2', name: 'Beta' });

const baseProps = {
  ...noopHandlers,
  accounts: [offlineAccount()],
  activeAccount: offlineAccount(),
  instances: [instA, instB],
  activeInstance: instA,
  running: null,
  installing: false,
};

const dotId = (id: string) => `sidebar-instance-running-dot-${id}`;
const stopId = (id: string) => `sidebar-stop-instance-${id}`;

async function openInstancePicker() {
  await fireEvent.click(screen.getByRole('combobox', { name: /instance/i }));
}

describe('Sidebar per-instance running indicators', () => {
  beforeEach(() => initSidebarButtons([]));

  it('shows a running badge only on the running row', async () => {
    render(Sidebar, { props: { ...baseProps, isRunning: (id: string) => id === 'inst-2' } });
    await openInstancePicker();
    expect(screen.getByTestId(dotId('inst-2'))).toBeTruthy();
    expect(screen.queryByTestId(dotId('inst-1'))).toBeNull();
  });

  it('reveals an inline Stop button on the running row (labelled Stop)', async () => {
    render(Sidebar, { props: { ...baseProps, isRunning: (id: string) => id === 'inst-2' } });
    await openInstancePicker();
    const stop = screen.getByTestId(stopId('inst-2'));
    expect(stop.getAttribute('aria-label')).toBe('Stop');
    expect(stop.querySelector('svg')).not.toBeNull();
    // No Stop control on the idle row.
    expect(screen.queryByTestId(stopId('inst-1'))).toBeNull();
  });

  it('inline Stop stops that instance without selecting/switching the row', async () => {
    const onStopInstance = vi.fn();
    const onSelectInstance = vi.fn();
    render(Sidebar, {
      props: {
        ...baseProps,
        onStopInstance,
        onSelectInstance,
        isRunning: (id: string) => id === 'inst-2',
      },
    });
    await openInstancePicker();
    const stop = screen.getByTestId(stopId('inst-2'));
    // mousedown is the row's commit trigger; Stop must swallow it so the click
    // stops rather than selects (mirrors the account-trash contract).
    await fireEvent.mouseDown(stop);
    await fireEvent.click(stop);
    expect(onStopInstance).toHaveBeenCalledWith('inst-2');
    expect(onSelectInstance).not.toHaveBeenCalled();
  });

  // Reactivity guard — the whole point of the escalation note: the badge must
  // clear the instant the page's `running` SvelteMap drops the id (process
  // exit), read live across the component boundary via the isRunning function
  // prop. A stale badge would falsely advertise a dead instance as running.
  it('badge disappears reactively when the instance leaves the running map', async () => {
    const running = new SvelteMap<string, { pid: number; version_id: string }>();
    running.set('inst-2', { pid: 1234, version_id: '1.20.4' });
    render(Sidebar, { props: { ...baseProps, isRunning: (id: string) => running.has(id) } });
    await openInstancePicker();
    expect(screen.getByTestId(dotId('inst-2'))).toBeTruthy();
    expect(screen.getByTestId(stopId('inst-2'))).toBeTruthy();

    // Simulate the process exiting: drop it from the reactive map.
    running.delete('inst-2');

    await waitFor(() => expect(screen.queryByTestId(dotId('inst-2'))).toBeNull());
    expect(screen.queryByTestId(stopId('inst-2'))).toBeNull();
  });

  // Compact / mini mode is not an icon-only rail — it shrinks the window but
  // keeps the Select trigger, whose leading avatar is the only always-visible
  // instance icon. The same leading snippet renders both the trigger and the
  // open rows, so a running SELECTED instance shows its badge on the closed
  // trigger, no dropdown open required.
  it('shows the badge on the closed trigger for a running selected instance', () => {
    render(Sidebar, {
      props: {
        ...baseProps,
        activeInstance: instB,
        isRunning: (id: string) => id === 'inst-2',
      },
    });
    // Dropdown left closed: the only rendered instance icon is the trigger's.
    expect(screen.getByTestId(dotId('inst-2'))).toBeTruthy();
  });
});
