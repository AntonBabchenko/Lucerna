import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Account, InstanceWithStatus } from '$lib/ipc/bindings';
import Sidebar from '$lib/layout/Sidebar.svelte';
import { initSidebarButtons } from '$lib/layout/sidebar-buttons.svelte';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    accountSkin: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
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

const baseProps = {
  onSelectAccount: () => {},
  onRemoveAccount: () => {},
  onOpenCosmetics: () => {},
  onAddOffline: () => {},
  onSelectInstance: () => {},
  onOpenManage: () => {},
  onOpenMods: () => {},
  onOpenLogs: () => {},
  onOpenModpacks: () => {},
  onOpenLauncherImport: () => {},
  onOpenGallery: () => {},
  onOpenQuickJoin: () => {},
  onPlay: () => {},
  onStop: () => {},
  onInstall: () => {},
  accounts: [offlineAccount()],
  activeAccount: offlineAccount(),
  instances: [instance()],
  activeInstance: instance(),
  running: null,
  installing: false,
};

// 'servers' is intentionally absent: the bottom Servers button was replaced by
// the (non-hideable) mode switcher; its registry entry is removed in Task 9.
const ALL_IDS = [
  'account_actions',
  'manage',
  'mods',
  'quick_join',
  'browse_modpacks',
  'import_launcher',
  'gallery',
  'logs',
];

describe('Sidebar button visibility', () => {
  beforeEach(() => initSidebarButtons([]));

  it('renders every candidate button by default', () => {
    render(Sidebar, { props: baseProps });
    expect(screen.getByTestId('sidebar-open-modpacks')).toBeTruthy();
    expect(screen.getByTestId('sidebar-open-launcher-import')).toBeTruthy();
    expect(screen.getByTestId('sidebar-open-gallery')).toBeTruthy();
    expect(screen.getByTestId('sidebar-open-logs')).toBeTruthy();
    expect(screen.getByRole('button', { name: /add offline/i })).toBeTruthy();
  });

  it('hides a button whose id is in the hidden set', () => {
    initSidebarButtons(['browse_modpacks', 'gallery']);
    render(Sidebar, { props: baseProps });
    expect(screen.queryByTestId('sidebar-open-modpacks')).toBeNull();
    expect(screen.queryByTestId('sidebar-open-gallery')).toBeNull();
    expect(screen.getByTestId('sidebar-open-launcher-import')).toBeTruthy();
  });

  it('hides the account action buttons together', () => {
    initSidebarButtons(['account_actions']);
    render(Sidebar, { props: baseProps });
    expect(screen.queryByRole('button', { name: /add offline/i })).toBeNull();
  });

  it('forces the account-add buttons visible when there are no accounts, even if hidden', () => {
    // Dead-end guard: with account_actions hidden AND no accounts, the user
    // would otherwise have no way to sign in or add an offline account.
    initSidebarButtons(['account_actions']);
    render(Sidebar, { props: { ...baseProps, accounts: [], activeAccount: null } });
    expect(screen.getByRole('button', { name: /add offline/i })).toBeTruthy();
  });

  it('keeps core Settings even when every hideable button is hidden', () => {
    initSidebarButtons(ALL_IDS);
    render(Sidebar, { props: baseProps });
    expect(screen.getByRole('button', { name: /^settings$/i })).toBeTruthy();
    expect(screen.queryByTestId('sidebar-open-logs')).toBeNull();
    expect(screen.queryByTestId('sidebar-open-gallery')).toBeNull();
  });

  it('keeps Mods when only Manage is hidden (row does not fully collapse)', () => {
    initSidebarButtons(['manage']);
    render(Sidebar, { props: baseProps });
    expect(screen.getByRole('button', { name: /mods/i })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /manage/i })).toBeNull();
  });
});
