import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstanceWithStatus, VersionEntry } from '$lib/ipc/bindings';

const m = vi.hoisted(() => ({
  listForgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listFabricLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listQuiltLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  checkInstanceModCompat: vi
    .fn()
    .mockResolvedValue({ status: 'ok', data: { rows: [], loader_outcome: null } }),
  instanceMemoryBounds: vi.fn().mockResolvedValue({
    min_mb: 1024,
    max_mb: 8192,
    recommended_max_mb: 8192,
    step_mb: 256,
    ram_known: false,
  }),
  setActiveInstance: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceName: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceMemory: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceJvmArgs: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceLoader: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  changeInstanceMc: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  openInstanceFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  openImportedSourceFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  deleteInstance: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  createInstance: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { ...m },
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

import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';

function makeInstance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-1',
    name: 'Default',
    mc_version: '1.20.1',
    loader: 'vanilla',
    loader_version: null,
    max_heap_mb: 2048,
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

beforeEach(() => vi.clearAllMocks());

describe('ManageInstancesModal — name edit survives background refresh', () => {
  it('keeps an in-progress name edit when instances refresh with the same selection', async () => {
    const inst = makeInstance({ name: 'Default' });
    const { rerender } = render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });

    const input = (await screen.findByDisplayValue('Default')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'My New Name' } });

    const refreshed = makeInstance({ name: 'Default' });
    await rerender({
      open: true,
      instances: [refreshed],
      activeInstance: refreshed,
      versions: [version],
      onChanged: () => {},
    });

    expect(screen.getByDisplayValue('My New Name')).toBeTruthy();
  });

  it('resets the name draft when switching to a different instance', async () => {
    const a = makeInstance({ id: 'a', name: 'Alpha' });
    const b = makeInstance({ id: 'b', name: 'Beta' });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [a, b],
        activeInstance: a,
        versions: [version],
        onChanged: () => {},
      },
    });

    const input = (await screen.findByDisplayValue('Alpha')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'edited-but-not-committed' } });

    await fireEvent.click(screen.getByRole('button', { name: /Beta/ }));

    expect(screen.getByDisplayValue('Beta')).toBeTruthy();
  });

  it('discards an uncommitted name edit when the modal is closed and reopened on the same instance', async () => {
    const inst = makeInstance({ name: 'Default' });
    const baseProps = {
      instances: [inst],
      activeInstance: inst,
      versions: [version],
      onChanged: () => {},
    };
    const { rerender } = render(ManageInstancesModal, { props: { open: true, ...baseProps } });

    const input = (await screen.findByDisplayValue('Default')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'Unsaved' } });

    await rerender({ open: false, ...baseProps });
    await rerender({ open: true, ...baseProps });

    expect(await screen.findByDisplayValue('Default')).toBeTruthy();
  });
});

describe('ManageInstancesModal — memory slider', () => {
  it('persists memory only on release (change), not on every drag tick (input)', async () => {
    const inst = makeInstance({ max_heap_mb: 2048 });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });

    const slider = (await screen.findByRole('slider')) as HTMLInputElement;

    await fireEvent.input(slider, { target: { value: '4096' } });
    expect(m.setInstanceMemory).not.toHaveBeenCalled();

    await fireEvent.change(slider, { target: { value: '4096' } });
    expect(m.setInstanceMemory).toHaveBeenCalledTimes(1);
    expect(m.setInstanceMemory).toHaveBeenCalledWith('inst-1', 4096);
  });

  it('shows a heap above the fallback max once real bounds load (thumb tracks)', async () => {
    m.instanceMemoryBounds.mockResolvedValueOnce({
      min_mb: 1024,
      max_mb: 24576,
      recommended_max_mb: 16384,
      step_mb: 256,
      ram_known: true,
    });
    const inst = makeInstance({ max_heap_mb: 12288 });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });

    const slider = (await screen.findByRole('slider')) as HTMLInputElement;
    await waitFor(() => expect(slider.max).toBe('24576'));
    expect(slider.value).toBe('12288');
  });
});

describe('ManageInstancesModal — running guard', () => {
  function renderTwo(isRunning: boolean) {
    const a = makeInstance({ id: 'a', name: 'Alpha' });
    const b = makeInstance({ id: 'b', name: 'Beta' });
    return render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [a, b],
        activeInstance: a,
        versions: [version],
        onChanged: () => {},
        isRunning,
      },
    });
  }

  it('disables delete, MC version, and loader picker while running', async () => {
    renderTwo(true);
    await screen.findByDisplayValue('Alpha');

    expect(
      (screen.getByRole('button', { name: /Delete instance/ }) as HTMLButtonElement).disabled,
    ).toBe(true);
    expect((screen.getByRole('combobox') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: /vanilla/i }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it('leaves delete, MC version, and loader picker enabled when not running', async () => {
    renderTwo(false);
    await screen.findByDisplayValue('Alpha');

    expect(
      (screen.getByRole('button', { name: /Delete instance/ }) as HTMLButtonElement).disabled,
    ).toBe(false);
    expect((screen.getByRole('combobox') as HTMLButtonElement).disabled).toBe(false);
    expect((screen.getByRole('button', { name: /vanilla/i }) as HTMLButtonElement).disabled).toBe(
      false,
    );
  });
});
