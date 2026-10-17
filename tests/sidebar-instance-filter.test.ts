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

const account: Account = {
  id: 'of-1',
  kind: 'offline',
  name: 'Steve',
  uuid: '00000000-0000-0000-0000-000000000001',
  expires_at: null,
};

function instance(id: string, name: string): InstanceWithStatus {
  return {
    id,
    name,
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
  };
}

const NAMES = [
  'Survival',
  'Skyblock',
  'Creative',
  'Hardcore',
  'Modded',
  'Vanilla',
  'Parkour',
  'Builds',
  'Friends',
];
const many = (n: number) => NAMES.slice(0, n).map((name, i) => instance(`inst-${i}`, name));

function mount(instances: InstanceWithStatus[]) {
  const onSelectInstance = vi.fn();
  render(Sidebar, {
    props: {
      onSelectAccount: () => {},
      onRemoveAccount: () => {},
      onOpenCosmetics: () => {},
      onAddOffline: () => {},
      onSelectInstance,
      onOpenMods: () => {},
      onOpenLogs: () => {},
      onOpenModpacks: () => {},
      onOpenLauncherImport: () => {},
      onOpenGallery: () => {},
      onOpenQuickJoin: () => {},
      onPlay: () => {},
      onStop: () => {},
      onInstall: () => {},
      onCreateInstance: () => {},
      accounts: [account],
      activeAccount: account,
      running: null,
      installing: false,
      instances,
      activeInstance: instances[0],
    },
  });
  return { onSelectInstance, trigger: screen.getByRole('combobox', { name: /^instance$/i }) };
}

const filterField = () => screen.queryByRole('combobox', { name: 'Filter instances by name' });

describe('Sidebar instance dropdown — the name filter', () => {
  beforeEach(() => initSidebarButtons([]));

  it('offers no filter for eight instances or fewer', async () => {
    const { trigger } = mount(many(8));
    await fireEvent.click(trigger);
    expect(screen.getAllByRole('option')).toHaveLength(8);
    expect(filterField()).toBeNull();
  });

  it('above eight, typing a name picks that instance from the keyboard', async () => {
    const { trigger, onSelectInstance } = mount(many(9));
    trigger.focus();
    await fireEvent.keyDown(trigger, { key: 'p' });
    const field = filterField() as HTMLInputElement;
    expect(field.placeholder).toBe('Filter instances…');
    await fireEvent.input(field, { target: { value: 'park' } });
    expect(screen.getAllByRole('option').map((o) => o.textContent?.trim())).toEqual([
      expect.stringContaining('Parkour'),
    ]);
    await fireEvent.keyDown(field, { key: 'Enter' });
    expect(onSelectInstance).toHaveBeenCalledWith('inst-6');
  });

  it('matches the name only, and says so when nothing matches', async () => {
    const { trigger } = mount(many(9));
    await fireEvent.click(trigger);
    // Every row reads «… · Fabric 1.20.4»; the loader is not part of the name.
    await fireEvent.input(filterField() as HTMLInputElement, { target: { value: 'fabric' } });
    expect(screen.queryAllByRole('option')).toHaveLength(0);
    expect(screen.getByRole('status').textContent).toBe('No instance name contains “fabric”.');
  });
});
