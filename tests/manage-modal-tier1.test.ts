import { fireEvent, render, screen, within } from '@testing-library/svelte';
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

    // Close via the real close button (runs close(), which clears the resync
    // cursor) — toggling the `open` prop alone would bypass close().
    await fireEvent.click(screen.getByRole('button', { name: /close manage instances/i }));
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
    expect((screen.getByRole('button', { name: 'Vanilla' }) as HTMLButtonElement).disabled).toBe(
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
    expect((screen.getByRole('button', { name: 'Vanilla' }) as HTMLButtonElement).disabled).toBe(
      false,
    );
  });
});

describe('ManageInstancesModal — list & sidebar usability', () => {
  function makeMany(n: number): InstanceWithStatus[] {
    return Array.from({ length: n }, (_, idx) =>
      makeInstance({ id: `inst-${idx}`, name: `Profile ${idx}` }),
    );
  }

  it('pins the New-instance button ahead of the scrollable list', async () => {
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
    await screen.findByDisplayValue('Alpha');

    const createBtn = screen.getByRole('button', { name: '+ New instance' });
    const row = screen.getByRole('button', { name: /Beta/ });
    // The create button must come BEFORE the list rows in DOM order so it stays
    // pinned while the list scrolls.
    expect(createBtn.compareDocumentPosition(row) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('renders long instance names in a truncating span', async () => {
    const longName = 'A really really long instance name that overflows the sidebar';
    const inst = makeInstance({ id: 'a', name: longName });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });
    await screen.findByDisplayValue(longName);

    const truncating = screen
      .getAllByText(longName)
      .filter((el) => el.className.includes('truncate'));
    expect(truncating.length).toBeGreaterThan(0);
  });

  it('marks the active instance row with an Active chip and leaves others unmarked', async () => {
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
    await screen.findByDisplayValue('Alpha');

    const alphaRow = screen.getByRole('button', { name: /Alpha/ });
    const betaRow = screen.getByRole('button', { name: /Beta/ });
    expect(within(alphaRow).getByText('Active')).toBeTruthy();
    expect(within(betaRow).queryByText('Active')).toBeNull();
  });

  it('hides the filter below the threshold and shows it once the list grows', async () => {
    const few = makeMany(3);
    const { rerender } = render(ManageInstancesModal, {
      props: {
        open: true,
        instances: few,
        activeInstance: few[0],
        versions: [version],
        onChanged: () => {},
      },
    });
    await screen.findByDisplayValue('Profile 0');
    expect(screen.queryByPlaceholderText(/filter/i)).toBeNull();

    const many = makeMany(9);
    await rerender({
      open: true,
      instances: many,
      activeInstance: many[0],
      versions: [version],
      onChanged: () => {},
    });
    expect(screen.getByPlaceholderText(/filter/i)).toBeTruthy();
  });

  it('filters rows by name and shows a no-matches note', async () => {
    const many = makeMany(9); // Profile 0 .. Profile 8
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: many,
        activeInstance: many[0],
        versions: [version],
        onChanged: () => {},
      },
    });
    const filter = await screen.findByPlaceholderText(/filter/i);

    await fireEvent.input(filter, { target: { value: 'Profile 3' } });
    expect(screen.getByRole('button', { name: /Profile 3/ })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /Profile 4/ })).toBeNull();

    await fireEvent.input(filter, { target: { value: 'zzz-no-such-name' } });
    expect(screen.getByText('No instances match.')).toBeTruthy();
  });

  it('clears the filter after the modal is closed', async () => {
    const many = makeMany(9);
    const baseProps = {
      instances: many,
      activeInstance: many[0],
      versions: [version],
      onChanged: () => {},
    };
    const { rerender } = render(ManageInstancesModal, { props: { open: true, ...baseProps } });

    const filter = (await screen.findByPlaceholderText(/filter/i)) as HTMLInputElement;
    await fireEvent.input(filter, { target: { value: 'Profile 3' } });
    expect(filter.value).toBe('Profile 3');

    // Close via the real close button so close() runs and resets the filter.
    await fireEvent.click(screen.getByRole('button', { name: /close manage instances/i }));
    await rerender({ open: true, ...baseProps });

    const reopened = (await screen.findByPlaceholderText(/filter/i)) as HTMLInputElement;
    expect(reopened.value).toBe('');
  });
});
