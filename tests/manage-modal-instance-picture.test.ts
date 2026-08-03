import { render, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { InstanceWithStatus, VersionEntry } from '$lib/ipc/bindings';

const m = vi.hoisted(() => ({
  instancePathStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'ok' }),
  previewInstanceDirName: vi.fn().mockResolvedValue('Preview-Name'),
  listForgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listFabricLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listQuiltLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
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
      default_mb: 2048,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    }),
    setActiveInstance: vi.fn(),
    setInstanceName: vi.fn(),
    setInstanceMemory: vi.fn(),
    setInstanceJvmArgs: vi.fn(),
    openInstanceFolder: vi.fn(),
    openImportedSourceFolder: vi.fn(),
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
vi.mock('$lib/servers/server-state.svelte', () => ({ serverState: { list: [] } }));

// The avatar's only IPC dependency. A resolved data: URL is what the real cache
// hands back for an instance that has a custom picture.
vi.mock('$lib/instances/instance-icon-cache', () => ({
  loadInstanceIcon: vi.fn().mockResolvedValue('data:image/png;base64,AAAA'),
  invalidateInstanceIcon: vi.fn(),
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
    has_icon: true,
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

function renderModal(instances: InstanceWithStatus[]) {
  return render(ManageInstancesModal, {
    props: {
      open: true,
      instances,
      activeInstance: instances[0],
      versions: [version],
      onChanged: () => {},
    },
  });
}

// The avatar is the only real <img> in the modal — the status/warning glyphs are
// inline <svg role="img">. Counting <img> therefore isolates pictures without
// pinning class recipes or locale strings.
const picturesIn = (selector: string) =>
  document.body.querySelector(selector)?.querySelectorAll('img').length ?? -1;

describe('ManageInstancesModal — instance picture', () => {
  it('shows a picture for every instance in the list', async () => {
    renderModal([makeInstance(), makeInstance({ id: 'inst-2', name: 'Skyblock' })]);
    await waitFor(() => expect(picturesIn('aside')).toBe(2));
  });

  it("shows the selected instance's picture in the detail pane", async () => {
    renderModal([makeInstance()]);
    await waitFor(() => expect(picturesIn('section')).toBe(1));
  });

  it('falls back to the letter avatar when the instance has no picture', async () => {
    renderModal([makeInstance({ has_icon: false })]);
    // Detail pane still renders (the name field proves it), but with no <img>.
    await waitFor(() => expect(document.body.querySelector('#detail-name')).toBeTruthy());
    expect(picturesIn('section')).toBe(0);
    expect(picturesIn('aside')).toBe(0);
  });
});
