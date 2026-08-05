import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Account, InstanceWithStatus } from '$lib/ipc/bindings';
import Sidebar from '$lib/layout/Sidebar.svelte';
import { initSidebarButtons } from '$lib/layout/sidebar-buttons.svelte';

vi.mock('$lib/ipc/bindings', () => ({
  commands: { accountSkin: vi.fn().mockResolvedValue({ status: 'ok', data: null }) },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    processExited: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn().mockResolvedValue(null) }));
vi.mock('@tauri-apps/api/core', () => ({ Channel: vi.fn() }));

function offlineAccount(): Account {
  return {
    id: 'of-1',
    kind: 'offline',
    name: 'Steve',
    uuid: '00000000-0000-0000-0000-000000000001',
    expires_at: null,
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
  accounts: [offlineAccount()],
  activeAccount: offlineAccount(),
  instances: [instance()],
  activeInstance: instance(),
  running: null,
  installing: false,
};

describe('Sidebar per-instance right-click menu', () => {
  beforeEach(() => initSidebarButtons([]));

  it('right-click on a row opens the menu; Clone reports the ROW id', async () => {
    const onCloneInstance = vi.fn();
    const alt = instance({ id: 'inst-2', name: 'Other' });
    render(Sidebar, {
      props: {
        ...baseProps,
        onCloneInstance,
        instances: [instance(), alt], // active = inst-1
        activeInstance: instance(),
      },
    });

    await fireEvent.click(screen.getByRole('combobox', { name: /instance/i }));
    const row = screen.getByRole('option', { name: /other/i });
    await fireEvent.contextMenu(row);

    const item = screen.getByTestId('sidebar-ctx-clone-instance');
    await fireEvent.click(item);

    expect(onCloneInstance).toHaveBeenCalledTimes(1);
    expect(onCloneInstance).toHaveBeenCalledWith('inst-2');
    // Menu closes after the action (Menu calls onClose before onSelect —
    // the captured id above is the regression this pins).
    expect(screen.queryByTestId('sidebar-ctx-clone-instance')).toBeNull();
  });

  it('right-click does not commit / switch the row', async () => {
    const onSelectInstance = vi.fn();
    const alt = instance({ id: 'inst-2', name: 'Other' });
    render(Sidebar, {
      props: {
        ...baseProps,
        onSelectInstance,
        onCloneInstance: () => {},
        instances: [instance(), alt],
        activeInstance: instance(),
      },
    });

    await fireEvent.click(screen.getByRole('combobox', { name: /instance/i }));
    await fireEvent.contextMenu(screen.getByRole('option', { name: /other/i }));

    expect(onSelectInstance).not.toHaveBeenCalled();
  });
});
