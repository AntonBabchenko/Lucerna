import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
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
  setActiveInstance: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  createInstance: vi.fn().mockResolvedValue({ status: 'ok', data: { id: 'new-id', name: 'New' } }),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { ...m },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

// A 16 GB machine: the adaptive default the backend would assign is 6 GB.
// Mocking the module (rather than the command) also bypasses the session-level
// cache inside memory-bounds.ts, so each test gets a deterministic seed.
const { mockLoad } = vi.hoisted(() => ({ mockLoad: vi.fn() }));
const RAM_16GB = {
  min_mb: 1024,
  max_mb: 16384,
  default_mb: 6144,
  recommended_max_mb: 12288,
  step_mb: 256,
  ram_known: true,
};
vi.mock('$lib/instances/memory-bounds', () => ({
  FALLBACK_MEMORY_BOUNDS: {
    min_mb: 1024,
    max_mb: 8192,
    default_mb: 2048,
    recommended_max_mb: 8192,
    step_mb: 256,
    ram_known: false,
  },
  loadMemoryBounds: mockLoad,
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/webview', () => ({}));
vi.mock('$lib/servers/server-state.svelte', () => ({ serverState: { list: [] } }));

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

const version: VersionEntry = {
  id: '1.20.1',
  version_type: 'release',
  release_date: '2024-01-01T00:00:00+00:00',
  url: '',
};

function renderModal() {
  const inst = makeInstance();
  return render(ManageInstancesModal, {
    props: {
      open: true,
      instances: [inst],
      activeInstance: inst,
      versions: [version],
      onChanged: () => {},
    },
  });
}

/** Open the create form and fill the two fields Create validates. */
async function openCreateForm() {
  await fireEvent.click(screen.getByRole('button', { name: '+ New instance' }));
  await fireEvent.input(screen.getByLabelText(/name/i), { target: { value: 'My Instance' } });
  // The MC version uses the custom Select — options commit on mousedown.
  await fireEvent.click(screen.getByRole('combobox'));
  await fireEvent.mouseDown(screen.getByRole('option', { name: '1.20.1' }));
}

describe('create-instance form — memory picker', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    vi.clearAllMocks();
    mockLoad.mockResolvedValue(RAM_16GB);
  });

  it('seeds the slider with the adaptive default for this machine', async () => {
    renderModal();
    await openCreateForm();

    const slider = screen.getByRole('slider') as HTMLInputElement;
    await waitFor(() => expect(slider.max).toBe('16384'));
    expect(slider.value).toBe('6144');
    expect((screen.getByRole('spinbutton') as HTMLInputElement).value).toBe('6144');
  });

  it('creates the instance with the heap the user picked, not the default', async () => {
    renderModal();
    await openCreateForm();

    const slider = screen.getByRole('slider') as HTMLInputElement;
    await waitFor(() => expect(slider.max).toBe('16384'));
    await fireEvent.input(slider, { target: { value: '8192' } });

    await fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    await waitFor(() => expect(m.createInstance).toHaveBeenCalledTimes(1));
    expect(m.createInstance).toHaveBeenCalledWith('My Instance', '1.20.1', 'vanilla', null, 8192);
  });

  it('sends the adaptive default when the user never touches the slider', async () => {
    renderModal();
    await openCreateForm();
    await waitFor(() => expect((screen.getByRole('slider') as HTMLInputElement).max).toBe('16384'));

    await fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    await waitFor(() => expect(m.createInstance).toHaveBeenCalledTimes(1));
    expect(m.createInstance).toHaveBeenCalledWith('My Instance', '1.20.1', 'vanilla', null, 6144);
  });

  it('forgets the previous choice when the create form is reopened', async () => {
    renderModal();
    await openCreateForm();

    const slider = screen.getByRole('slider') as HTMLInputElement;
    await waitFor(() => expect(slider.max).toBe('16384'));
    await fireEvent.input(slider, { target: { value: '12288' } });
    expect((screen.getByRole('spinbutton') as HTMLInputElement).value).toBe('12288');

    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await fireEvent.click(screen.getByRole('button', { name: '+ New instance' }));

    expect((screen.getByRole('spinbutton') as HTMLInputElement).value).toBe('6144');
  });
});
