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
    java: null,
    ...over,
  };
}
const baseProps = {
  onSelectAccount: () => {},
  onRemoveAccount: () => {},
  onOpenCosmetics: () => {},
  onAddOffline: () => {},
  onSelectInstance: () => {},
  onCreateInstance: () => {},
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

describe('Sidebar per-instance settings icon', () => {
  beforeEach(() => initSidebarButtons([]));

  it('asks for the CLICKED profile, not the active one', async () => {
    // Regression guard: the Settings tab must open on the row the user clicked,
    // even when it is not the active instance. The active switch is async, so
    // the request names the clicked id and waits for its activation.
    const onOpenInstanceSettings = vi.fn();
    const onSelectInstance = vi.fn();
    const alt = instance({ id: 'inst-2', name: 'Other' });
    render(Sidebar, {
      props: {
        ...baseProps,
        onOpenInstanceSettings,
        onSelectInstance,
        instances: [instance(), alt], // active = inst-1
        activeInstance: instance(),
      },
    });
    // Open the profile dropdown, then press the NON-active row's settings icon.
    // The Select commits option rows on mousedown, so the icon is driven the
    // same way: its mousedown bubbles to the row, which makes it active.
    await fireEvent.click(screen.getByRole('combobox', { name: /instance/i }));
    await fireEvent.mouseDown(screen.getByTestId('sidebar-instance-settings-inst-2'));
    expect(onOpenInstanceSettings).toHaveBeenCalledTimes(1);
    expect(onOpenInstanceSettings).toHaveBeenCalledWith('inst-2');
    expect(onSelectInstance).toHaveBeenCalledWith('inst-2');
  });
});
