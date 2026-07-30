import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { InstanceWithStatus, VersionEntry } from '$lib/ipc/bindings';

const m = vi.hoisted(() => ({
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
vi.mock('$lib/instances/instance-icon-cache', () => ({
  loadInstanceIcon: vi.fn().mockResolvedValue(null),
  invalidateInstanceIcon: vi.fn(),
}));

import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';

const NAME = 'Skyblock Deluxe';

function makeInstance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-1',
    name: NAME,
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
  const instance = makeInstance();
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

const splitter = () => document.body.querySelector('[data-testid="manage-list-splitter"]');
const listWidth = () =>
  Number.parseInt(
    (document.body.querySelector('aside') as HTMLElement)?.style.width.replace('px', '') ?? '0',
    10,
  );

describe('ManageInstancesModal — layout', () => {
  it('does not repeat the instance name as a heading above the name field', async () => {
    const { findByDisplayValue } = renderModal();
    // The detail pane is up (the name field holds the value)...
    await findByDisplayValue(NAME);
    // ...and no heading duplicates it.
    const headings = [...document.body.querySelectorAll('h1,h2,h3,h4')].map((h) => h.textContent);
    expect(headings.some((text) => text?.includes(NAME))).toBe(false);
  });

  it('exposes the list splitter with its resize bounds', async () => {
    renderModal();
    await waitFor(() => expect(splitter()).toBeTruthy());
    const el = splitter() as HTMLElement;
    expect(el.getAttribute('aria-orientation')).toBe('vertical');
    expect(el.getAttribute('aria-valuemin')).toBe('180');
    expect(el.getAttribute('aria-valuemax')).toBe('420');
    expect(el.getAttribute('aria-valuenow')).toBe(String(listWidth()));
  });

  it('widens the list on ArrowRight and narrows it on ArrowLeft', async () => {
    renderModal();
    await waitFor(() => expect(splitter()).toBeTruthy());
    const el = splitter() as HTMLElement;
    const before = listWidth();

    await fireEvent.keyDown(el, { key: 'ArrowRight' });
    expect(listWidth()).toBeGreaterThan(before);

    await fireEvent.keyDown(el, { key: 'ArrowLeft' });
    expect(listWidth()).toBe(before);
  });

  it('clamps the list at its minimum width', async () => {
    renderModal();
    await waitFor(() => expect(splitter()).toBeTruthy());
    const el = splitter() as HTMLElement;
    for (let i = 0; i < 40; i++) await fireEvent.keyDown(el, { key: 'ArrowLeft' });
    expect(listWidth()).toBe(180);
  });
});
