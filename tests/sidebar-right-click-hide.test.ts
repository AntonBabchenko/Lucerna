import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Account, InstanceWithStatus } from '$lib/ipc/bindings';
import Sidebar from '$lib/layout/Sidebar.svelte';
import { initSidebarButtons } from '$lib/layout/sidebar-buttons.svelte';

const { appSettingsGet, appSettingsSetGeneral } = vi.hoisted(() => ({
  appSettingsGet: vi.fn().mockResolvedValue({
    status: 'ok',
    data: { general: { hidden_sidebar_buttons: [] } },
  }),
  appSettingsSetGeneral: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    accountSkin: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    appSettingsGet,
    appSettingsSetGeneral,
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
  onAddOffline: () => {},
  onSelectInstance: () => {},
  onOpenManage: () => {},
  onOpenMods: () => {},
  onOpenLogs: () => {},
  onOpenModpacks: () => {},
  onOpenLauncherImport: () => {},
  onOpenServers: () => {},
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

describe('Sidebar right-click hide', () => {
  beforeEach(() => {
    initSidebarButtons([]);
    appSettingsGet.mockClear();
    appSettingsSetGeneral.mockClear();
  });

  it('right-clicking a hideable button surfaces a Hide menu item', async () => {
    render(Sidebar, { props: baseProps });
    await fireEvent.contextMenu(screen.getByTestId('sidebar-open-servers'));
    expect(screen.getByTestId('sidebar-ctx-hide-servers')).toBeTruthy();
  });

  it('choosing Hide opens the confirm dialog; confirming persists the hidden id', async () => {
    render(Sidebar, { props: baseProps });
    await fireEvent.contextMenu(screen.getByTestId('sidebar-open-servers'));
    await fireEvent.click(screen.getByTestId('sidebar-ctx-hide-servers'));
    expect(screen.getByTestId('hide-button-confirm')).toBeTruthy();
    await fireEvent.click(screen.getByTestId('hide-button-confirm'));
    expect(appSettingsSetGeneral).toHaveBeenCalledTimes(1);
    expect(appSettingsSetGeneral.mock.calls[0][0]).toMatchObject({
      hidden_sidebar_buttons: ['servers'],
    });
  });

  it('cancelling the dialog leaves the button visible and persists nothing', async () => {
    render(Sidebar, { props: baseProps });
    await fireEvent.contextMenu(screen.getByTestId('sidebar-open-servers'));
    await fireEvent.click(screen.getByTestId('sidebar-ctx-hide-servers'));
    await fireEvent.click(screen.getByRole('button', { name: /cancel/i }));
    expect(screen.queryByTestId('hide-button-confirm')).toBeNull();
    expect(appSettingsSetGeneral).not.toHaveBeenCalled();
    expect(screen.getByTestId('sidebar-open-servers')).toBeTruthy();
  });

  it('refuses to hide the account buttons with no accounts, explaining instead', async () => {
    render(Sidebar, { props: { ...baseProps, accounts: [], activeAccount: null } });
    await fireEvent.contextMenu(screen.getByRole('button', { name: /add offline/i }));
    await fireEvent.click(screen.getByTestId('sidebar-ctx-hide-account_actions'));
    // The account-required explainer shows; the normal hide-confirm does not,
    // and nothing is persisted (the buttons stay, so the user can still sign in).
    expect(screen.getByTestId('account-required-ok')).toBeTruthy();
    expect(screen.queryByTestId('hide-button-confirm')).toBeNull();
    expect(appSettingsSetGeneral).not.toHaveBeenCalled();
  });
});
