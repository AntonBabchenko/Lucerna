import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { InstanceWithStatus, ModSource, VersionEntry } from '$lib/ipc/bindings';

// The Settings tab renders LoaderPicker, which calls commands.list*Loaders
// when loader !== 'vanilla'. We use vanilla in all fixtures here so no loader
// IPC fires, but the module still needs to be mockable.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    instancePathStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'ok' }),
    previewInstanceDirName: vi.fn().mockResolvedValue('Preview-Name'),
    // ImportedDetailDrawer calls these at mount
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsProject: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { summary: { name: 'x' }, description: '', website_url: null },
    }),
    modpackStatus: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modpackUpdateStatus: vi.fn().mockResolvedValue({ status: 'ok', data: { kind: 'up_to_date' } }),
    // The Settings tab's memory slider fetches its bounds on mount
    instanceMemoryBounds: vi.fn().mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      default_mb: 2048,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    }),
    // The Settings tab calls these on user interaction only; mocked
    // here so imports don't throw at resolve time.
    createInstance: vi.fn(),
    setActiveInstance: vi.fn(),
    setInstanceName: vi.fn(),
    setInstanceVersion: vi.fn(),
    setInstanceLoader: vi.fn(),
    setInstanceMemory: vi.fn(),
    setInstanceJvmArgs: vi.fn(),
    openInstanceFolder: vi.fn(),
    deleteInstance: vi.fn(),
    // LoaderPicker loader-version fetchers (not called for vanilla)
    listFabricLoaders: vi.fn(),
    listQuiltLoaders: vi.fn(),
    listForgeLoaders: vi.fn(),
    listNeoforgeLoaders: vi.fn(),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import InstanceSettingsTab from '$lib/instances/settings/InstanceSettingsTab.svelte';
import ImportedDetailDrawer from '$lib/modpacks/ImportedDetailDrawer.svelte';

function makeInstance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
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

function makeVersion(id = '1.20.4'): VersionEntry {
  return { id, version_type: 'release', release_date: '2024-01-01T00:00:00+00:00', url: '' };
}

// Two instances, so Delete is not blocked as the last one.
const settingsProps = {
  instance: makeInstance(),
  instances: [makeInstance(), makeInstance({ id: 'inst-2', name: 'Second' })],
  instancesLoaded: true,
  listError: null,
  fellBack: false,
  versions: [makeVersion()],
  running: false,
  anyRunning: false,
  onChanged: () => {},
  onCreate: () => {},
  onDelete: () => {},
  onClone: () => {},
  onTranslations: () => {},
  onOpenFolder: () => {},
};

// The Settings tab puts Delete in a Danger zone of its own, after everything
// else — the server Settings tab's pattern — not beside harmless buttons.
describe('Settings tab — Danger zone', () => {
  it('Delete instance is btn-danger btn-sm, alone in the last section', () => {
    render(InstanceSettingsTab, { props: settingsProps });
    const deleteBtn = screen.getByTestId('settings-delete-instance');
    expect(deleteBtn).toHaveBtnVariant('danger');
    expect(deleteBtn).toHaveBtnSize('sm');
    const zone = deleteBtn.closest('[data-tour-ctx="settings-danger"]') as HTMLElement;
    expect(zone.querySelectorAll('button')).toHaveLength(1);
    // Nothing of the tab follows the zone.
    expect(zone.nextElementSibling).toBeNull();
  });
});

describe('ImportedDetailDrawer — destructive footer pattern', () => {
  it('Delete pack is btn-ghost-danger (LEFT) and Open instance is btn-primary (RIGHT)', () => {
    render(ImportedDetailDrawer, {
      props: {
        inst: makeInstance({
          id: 'pack-inst',
          mrpack_name: 'Cool Pack',
          mrpack_version: '1.0',
          mrpack_source: 'modrinth' as ModSource,
        }),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    const deleteBtn = screen.getByRole('button', { name: /delete pack/i });
    const openBtn = screen.getByRole('button', { name: /open instance/i });
    expect(deleteBtn).toHaveBtnVariant('ghost-danger');
    expect(openBtn).toHaveBtnVariant('primary');

    const footer = openBtn.closest('[class*="justify-between"]') as HTMLElement;
    expect(footer).not.toBeNull();
    const buttons = Array.from(footer.querySelectorAll<HTMLButtonElement>('button'));
    expect(buttons.indexOf(deleteBtn as HTMLButtonElement)).toBeLessThan(
      buttons.indexOf(openBtn as HTMLButtonElement),
    );
  });
});
