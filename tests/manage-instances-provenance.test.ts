import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { InstanceWithStatus, VersionEntry } from '$lib/ipc/bindings';

const m = vi.hoisted(() => ({
  listForgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listFabricLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listQuiltLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  openImportedSourceFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  checkInstanceModCompat: vi
    .fn()
    .mockResolvedValue({ status: 'ok', data: { rows: [], loader_outcome: null } }),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    ...m,
    instanceMemoryBounds: vi.fn().mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    }),
    setActiveInstance: vi.fn(),
    setInstanceName: vi.fn(),
    setInstanceMemory: vi.fn(),
    setInstanceJvmArgs: vi.fn(),
    openInstanceFolder: vi.fn(),
    deleteInstance: vi.fn(),
    createInstance: vi.fn(),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/webview', () => ({}));

// serverState.list starts empty; the provenance row falls back to the raw id.
vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: { list: [] },
}));

import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';

function makeInstance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-1',
    name: 'Default',
    mc_version: '1.20.1',
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

const version: VersionEntry = {
  id: '1.20.1',
  version_type: 'release',
  release_date: '2024-01-01T00:00:00+00:00',
  url: '',
};

function renderModal(instance: InstanceWithStatus) {
  return render(ManageInstancesModal, {
    props: {
      open: true,
      instances: [instance],
      activeInstance: instance,
      versions: [version],
      onChanged: () => {},
    },
  });
}

describe('ManageInstancesModal — imported-instance provenance', () => {
  it('shows the provenance row + open-source-folder button and calls the command', async () => {
    const imported = makeInstance({
      imported_from: {
        launcher: 'tlauncher',
        source_name: 'test',
        source_path: 'C:/Users/x/AppData/Roaming/.minecraft/versions/test',
        imported_unix_ms: 0,
      },
    });
    renderModal(imported);

    await waitFor(() => expect(screen.getByTestId('imported-provenance')).toBeTruthy());
    const btn = screen.getByTestId('open-source-folder-btn');
    await fireEvent.click(btn);
    expect(m.openImportedSourceFolder).toHaveBeenCalledWith('inst-1');
  });

  it('hides the provenance row for a non-imported instance', async () => {
    renderModal(makeInstance({ imported_from: null }));
    // The detail panel renders for the selected instance (its name input is
    // unique); the provenance row must be absent.
    await waitFor(() => expect(screen.getByDisplayValue('Default')).toBeTruthy());
    expect(screen.queryByTestId('imported-provenance')).toBeNull();
  });
});

describe('ManageInstancesModal — created-from-server provenance', () => {
  it('shows the provenance row when created_from_server is set', async () => {
    const inst = makeInstance({ created_from_server: 'srv-abc' });
    renderModal(inst);

    await waitFor(() => expect(screen.getByTestId('created-from-server-provenance')).toBeTruthy());
    // serverState.list is empty so the row falls back to the raw id.
    expect(screen.getByTestId('created-from-server-provenance').textContent).toContain('srv-abc');
  });

  it('hides the provenance row when created_from_server is null', async () => {
    renderModal(makeInstance({ created_from_server: null }));
    await waitFor(() => expect(screen.getByDisplayValue('Default')).toBeTruthy());
    expect(screen.queryByTestId('created-from-server-provenance')).toBeNull();
  });
});
