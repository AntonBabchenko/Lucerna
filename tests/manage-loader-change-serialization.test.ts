// Spec D8: rapid loader clicks in the Manage modal must apply strictly in
// click order, one backend command at a time. The bug: each click fired its
// own concurrent setInstanceLoader (each resolves a recommended build —
// seconds); completions trickled back in arbitrary order for a while after
// the user stopped clicking, every onChanged() re-rendered the picker to
// that command's result («загрузчики сами переключаются»), and the FINAL
// on-disk loader was whichever command landed last, not the last click.

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstanceWithStatus, VersionEntry } from '$lib/ipc/bindings';

const m = vi.hoisted(() => ({
  instancePathStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'ok' }),
  previewInstanceDirName: vi.fn().mockResolvedValue('Preview-Name'),
  listForgeLoaders: vi
    .fn()
    .mockResolvedValue({ status: 'ok', data: [{ version: '47.2.0', stable: true }] }),
  listFabricLoaders: vi
    .fn()
    .mockResolvedValue({ status: 'ok', data: [{ version: '0.20.0', stable: true }] }),
  listQuiltLoaders: vi
    .fn()
    .mockResolvedValue({ status: 'ok', data: [{ version: '0.30.0', stable: true }] }),
  listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  setInstanceLoader: vi.fn(),
  detachInstancePack: vi.fn(),
  scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    ...m,
    instanceMemoryBounds: vi.fn().mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      default_mb: 2048,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    }),
    setActiveInstance: vi.fn(),
    setInstanceName: vi.fn(),
    setInstanceVersion: vi.fn(),
    setInstanceMemory: vi.fn(),
    setInstanceJvmArgs: vi.fn(),
    openInstanceFolder: vi.fn(),
    deleteInstance: vi.fn(),
    createInstance: vi.fn(),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/webview', () => ({}));

import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';
import { invalidateCompatScan } from '$lib/mods/compat-scan.svelte';

function makeInstance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-forge',
    name: 'Forge One',
    mc_version: '1.20.1',
    loader: 'forge',
    loader_version: '47.2.0',
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
  } as InstanceWithStatus;
}

const version: VersionEntry = {
  id: '1.20.1',
  version_type: 'release',
  release_date: '2024-01-01T00:00:00+00:00',
  url: '',
};

const okResult = (loader: string) => ({
  status: 'ok',
  data: makeInstance({ loader: loader as InstanceWithStatus['loader'] }),
});

describe('ManageInstancesModal — loader changes are serialized (spec D8)', () => {
  beforeEach(() => {
    m.setInstanceLoader.mockReset();
    m.scanInstanceModCompat.mockClear();
    invalidateCompatScan();
  });

  it('the last click wins: overtaken commands are silenced, queued older ones discarded', async () => {
    const inst = makeInstance();
    // Commands hang until we release them — the rapid-click window.
    const release: Array<(v: unknown) => void> = [];
    m.setInstanceLoader.mockImplementation(
      () =>
        new Promise((res) => {
          release.push(res);
        }),
    );
    const onChanged = vi.fn();

    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged,
      },
    });
    await waitFor(() => expect(m.listForgeLoaders).toHaveBeenCalled());

    // Click Fabric, then Quilt before the first command resolves.
    await fireEvent.click(screen.getByRole('button', { name: 'Fabric' }));
    await waitFor(() => expect(m.setInstanceLoader).toHaveBeenCalledTimes(1));
    await fireEvent.click(screen.getByRole('button', { name: 'Quilt' }));
    // A macrotask drain: the newer change must be QUEUED, not in flight.
    await new Promise((r) => setTimeout(r, 0));
    expect(m.setInstanceLoader).toHaveBeenCalledTimes(1);

    // The Fabric command lands — but it was overtaken, so it is silenced:
    // no onChanged (no picker flap toward a state the user clicked away
    // from), no rescan. The queued Quilt change starts immediately.
    release[0](okResult('fabric'));
    await waitFor(() => expect(m.setInstanceLoader).toHaveBeenCalledTimes(2));
    expect(m.setInstanceLoader.mock.calls[1][1]).toBe('quilt');
    expect(onChanged).not.toHaveBeenCalled();
    expect(m.scanInstanceModCompat).not.toHaveBeenCalled();

    // The final (last-clicked) change lands → exactly one onChanged and one
    // forced rescan. Any intermediate queued changes were discarded, so no
    // further commands run.
    release[1](okResult('quilt'));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
    await new Promise((r) => setTimeout(r, 0));
    await new Promise((r) => setTimeout(r, 0));
    expect(m.setInstanceLoader).toHaveBeenCalledTimes(2);
    expect(m.scanInstanceModCompat).toHaveBeenCalledTimes(1);
  });
});
