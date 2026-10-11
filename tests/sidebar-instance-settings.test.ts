import { fireEvent, render, screen, within } from '@testing-library/svelte';
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
    loader: 'fabric',
    loader_version: '0.15.0',
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
const handlers = () => ({
  onCreateInstance: vi.fn(),
  onOpenInstanceSettings: vi.fn(),
  onActivateInstance: vi.fn(),
  onRenameInstance: vi.fn(),
  onCloneInstance: vi.fn(),
  onTranslationsInstance: vi.fn(),
  onExportInstance: vi.fn(),
  onOpenInstanceFolder: vi.fn(),
  onOpenInstanceModsFolder: vi.fn(),
  onDeleteInstance: vi.fn(),
});
const baseProps = {
  onSelectAccount: () => {},
  onRemoveAccount: () => {},
  onOpenCosmetics: () => {},
  onAddOffline: () => {},
  onSelectInstance: () => {},
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
  running: null,
  installing: false,
};

const other = instance({ id: 'inst-2', name: 'Other' });

function mount(over: Record<string, unknown> = {}) {
  const h = handlers();
  render(Sidebar, {
    props: {
      ...baseProps,
      ...h,
      instances: [instance(), other],
      activeInstance: instance(),
      ...over,
    },
  });
  return h;
}

async function openList() {
  await fireEvent.click(screen.getByRole('combobox', { name: /instance/i }));
}

describe('Sidebar without the Manage window', () => {
  beforeEach(() => initSidebarButtons([]));

  it('has no Manage button any more', () => {
    mount();
    expect(screen.queryByRole('button', { name: 'Manage', exact: true })).toBeNull();
    expect(document.querySelector('[data-tour="manage-btn"]')).toBeNull();
  });

  it('offers New instance in the Content section', async () => {
    const h = mount();
    await fireEvent.click(screen.getByTestId('sidebar-new-instance'));
    expect(h.onCreateInstance).toHaveBeenCalledTimes(1);
  });

  it('disables New instance while the data folder is unavailable', () => {
    mount({ createBlockedReason: 'Data folder unavailable' });
    expect((screen.getByTestId('sidebar-new-instance') as HTMLButtonElement).disabled).toBe(true);
  });

  it('keeps the Content heading while New instance alone is visible', () => {
    initSidebarButtons(['browse_modpacks', 'import_launcher']);
    mount();
    expect(screen.getByTestId('sidebar-section-content')).toBeTruthy();
    expect(screen.getByTestId('sidebar-new-instance')).toBeTruthy();
  });

  it('drops the Content heading when all three of its buttons are hidden', () => {
    initSidebarButtons(['new_instance', 'browse_modpacks', 'import_launcher']);
    mount();
    expect(screen.queryByTestId('sidebar-section-content')).toBeNull();
  });

  it('the empty state creates through the same dialog', async () => {
    const h = mount({ instances: [], activeInstance: null });
    await fireEvent.click(screen.getByRole('button', { name: '+ Create' }));
    expect(h.onCreateInstance).toHaveBeenCalledTimes(1);
  });
});

describe('Sidebar instance list status', () => {
  beforeEach(() => initSidebarButtons([]));

  it('names integrity problems and a missing download, and says nothing for a healthy ready one', async () => {
    const broken = instance({
      id: 'inst-3',
      name: 'Broken',
      integrity: {
        healthy: false,
        problem_count: 2,
        checked_unix_ms: 1,
        categories: [],
      } as unknown as InstanceWithStatus['integrity'],
    });
    const fresh = instance({ id: 'inst-4', name: 'Fresh', ready: false });
    mount({ instances: [instance(), broken, fresh] });
    await openList();
    const list = screen.getByRole('listbox');
    expect(within(list).getByRole('img', { name: '2 integrity problems' })).toBeTruthy();
    expect(within(list).getByRole('img', { name: 'Download needed' })).toBeTruthy();
    const healthy = within(list).getByRole('option', { name: /Default/ });
    expect(within(healthy).queryByRole('img', { name: /integrity|download/i })).toBeNull();
  });

  it('the settings icon asks for that instance’s settings', async () => {
    const h = mount();
    await openList();
    await fireEvent.mouseDown(screen.getByTestId('sidebar-instance-settings-inst-2'));
    expect(h.onOpenInstanceSettings).toHaveBeenCalledWith('inst-2');
  });
});

describe('Sidebar row menu', () => {
  beforeEach(() => initSidebarButtons([]));

  it('offers every action for a modded, idle, non-active instance', async () => {
    mount();
    await openList();
    await fireEvent.contextMenu(screen.getByRole('option', { name: /Other/ }));
    for (const id of [
      'make-active',
      'rename',
      'clone-instance',
      'translations',
      'export',
      'open-folder',
      'open-mods-folder',
      'delete',
    ]) {
      expect(screen.getByTestId(`sidebar-ctx-${id}`)).toBeTruthy();
    }
  });

  it('Delete asks about the row, not the active instance', async () => {
    const h = mount();
    await openList();
    await fireEvent.contextMenu(screen.getByRole('option', { name: /Other/ }));
    await fireEvent.click(screen.getByTestId('sidebar-ctx-delete'));
    expect(h.onDeleteInstance).toHaveBeenCalledWith('inst-2');
  });

  it('a task on the instance blocks Delete with its reason', async () => {
    mount({ isBusy: (id: string) => id === 'inst-2' });
    await openList();
    await fireEvent.contextMenu(screen.getByRole('option', { name: /Other/ }));
    const del = screen.getByTestId('sidebar-ctx-delete') as HTMLButtonElement;
    expect(del.disabled).toBe(true);
    expect(del.textContent).toContain('Wait for the current task on this instance to finish.');
  });

  it('opens from the keyboard with an item active, and Escape returns focus to the list button', async () => {
    mount();
    const trigger = screen.getByRole('combobox', { name: /instance/i });
    trigger.focus();
    await fireEvent.keyDown(trigger, { key: 'ArrowDown' }); // opens on the active row
    await fireEvent.keyDown(trigger, { key: 'ArrowDown' }); // Other
    await fireEvent.keyDown(trigger, { key: 'F10', shiftKey: true });
    const menu = screen.getByRole('menu');
    // Opened from the keyboard: the first enabled item is highlighted (the
    // ContextMenu convention, tests/ui/cards/context-menu.test.ts).
    expect(screen.getAllByRole('menuitem')[0].classList.contains('bg-subtle')).toBe(true);
    expect(screen.queryByRole('listbox')).toBeNull();
    await fireEvent.keyDown(menu, { key: 'Escape' });
    await Promise.resolve();
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });
});
