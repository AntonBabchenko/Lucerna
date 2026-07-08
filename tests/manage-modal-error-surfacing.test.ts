import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstanceWithStatus, VersionEntry } from '$lib/ipc/bindings';

const m = vi.hoisted(() => ({
  listForgeLoaders: vi
    .fn()
    .mockResolvedValue({ status: 'ok', data: [{ version: '47.2.0', stable: true }] }),
  listFabricLoaders: vi
    .fn()
    .mockResolvedValue({ status: 'ok', data: [{ version: '0.16.0', stable: true }] }),
  listQuiltLoaders: vi
    .fn()
    .mockResolvedValue({ status: 'ok', data: [{ version: '0.30.0', stable: true }] }),
  listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
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
  setInstanceLoader: vi.fn().mockResolvedValue({
    status: 'ok',
    data: { id: 'inst-forge', mc_version: '1.20.1', loader: 'quilt' },
  }),
  changeInstanceMc: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  openInstanceFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  openImportedSourceFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  detachInstancePack: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
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

// Keep the real toast module's other exports, spy only the push helpers.
vi.mock('$lib/toasts/toasts.svelte', async (importOriginal) => {
  const actual = await importOriginal<typeof import('$lib/toasts/toasts.svelte')>();
  return { ...actual, pushSuccess: vi.fn(), pushWarning: vi.fn(), pushInfo: vi.fn() };
});

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/webview', () => ({}));
vi.mock('$lib/servers/server-state.svelte', () => ({ serverState: { list: [] } }));

import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';
import * as toasts from '$lib/toasts/toasts.svelte';

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

function renderOne(over: Partial<InstanceWithStatus> = {}) {
  const inst = makeInstance(over);
  render(ManageInstancesModal, {
    props: {
      open: true,
      instances: [inst],
      activeInstance: inst,
      versions: [version],
      onChanged: () => {},
    },
  });
  return inst;
}

beforeEach(() => vi.clearAllMocks());

describe('ManageInstancesModal — error surfacing', () => {
  it('surfaces a command failure in a role=alert live region', async () => {
    m.setInstanceName.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'instance_name_empty' },
    });
    renderOne({ name: 'Alpha' });

    const input = (await screen.findByDisplayValue('Alpha')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'Renamed' } });
    await fireEvent.blur(input);

    const msg = await screen.findByText(/name cannot be empty/i);
    expect(msg.closest('[role="alert"]')).not.toBeNull();
  });

  it('keeps an error live region present before any error (announce-on-change)', async () => {
    renderOne();
    await screen.findByDisplayValue('Default');
    // The modalError region (and the LoaderPicker error region) are always
    // mounted so a null→text transition is announced.
    expect(screen.getAllByRole('alert').length).toBeGreaterThan(0);
  });

  it('routes an open-folder failure to a toast, not the inline alert', async () => {
    m.openInstanceFolder.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: '/instances/x', details: 'permission denied' },
    });
    renderOne();
    await screen.findByDisplayValue('Default');

    await fireEvent.click(screen.getByRole('button', { name: 'Open folder' }));

    await waitFor(() => expect(vi.mocked(toasts.pushWarning)).toHaveBeenCalled());
    expect(vi.mocked(toasts.pushWarning).mock.calls[0][0]).toMatch(
      /couldn't open the instance folder/i,
    );
    // Not surfaced inline.
    expect(screen.queryByText(/couldn't open the instance folder/i)).toBeNull();
  });

  it('shows the compat summary in a polite (role=status) region after a loader change', async () => {
    // A loader switch runs the compat check twice — LoaderPicker fires `onchange`
    // on the click (old version) and again once it auto-resolves the new loader's
    // version. Use a stable mock (not ...Once) so both checks report the same
    // incompatibility; otherwise the second (resolved-version) check returns the
    // default empty list and clobbers the summary.
    m.checkInstanceModCompat.mockResolvedValue({
      status: 'ok',
      data: [{ sha1: 'a', name: 'X', status: { status: 'incompatible' } }],
    });
    renderOne({ id: 'inst-forge', name: 'Forge One', loader: 'forge', loader_version: '47.2.0' });
    await waitFor(() => expect(m.listForgeLoaders).toHaveBeenCalled());

    await fireEvent.click(screen.getByRole('button', { name: 'Quilt' }));

    const summary = await screen.findByText(/no version for this configuration/i);
    expect(summary.closest('[role="status"]')).not.toBeNull();
    expect(summary.closest('[role="alert"]')).toBeNull();
  });

  it('shows a quiet "couldn\'t check" note (role=status) when the compat check fails', async () => {
    // Stable mock (not ...Once): the loader switch runs the compat check twice, so
    // both attempts must fail for the "couldn't check" note to stay up — a single
    // ...Once would let the second (resolved-version) check succeed and clear it.
    m.checkInstanceModCompat.mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: '/x', details: 'boom' },
    });
    renderOne({ id: 'inst-forge', name: 'Forge One', loader: 'forge', loader_version: '47.2.0' });
    await waitFor(() => expect(m.listForgeLoaders).toHaveBeenCalled());

    await fireEvent.click(screen.getByRole('button', { name: 'Quilt' }));

    const note = await screen.findByText(/couldn't check mod compatibility/i);
    expect(note.closest('[role="status"]')).not.toBeNull();
    expect(note.closest('[role="alert"]')).toBeNull();
  });

  it('toasts that a newly created profile became active', async () => {
    m.createInstance.mockResolvedValueOnce({
      status: 'ok',
      data: { id: 'new-id', name: 'Fresh' },
    });
    renderOne();
    await screen.findByDisplayValue('Default');

    await fireEvent.click(screen.getByRole('button', { name: '+ New instance' }));
    await fireEvent.input(screen.getByLabelText(/name/i), { target: { value: 'Fresh' } });
    await fireEvent.click(screen.getByRole('combobox'));
    await fireEvent.mouseDown(screen.getByRole('option', { name: '1.20.1' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    await waitFor(() => expect(vi.mocked(toasts.pushSuccess)).toHaveBeenCalled());
    expect(vi.mocked(toasts.pushSuccess).mock.calls[0][0]).toMatch(/created fresh and switched/i);
  });
});
