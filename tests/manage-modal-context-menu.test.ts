import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstanceWithStatus, VersionEntry } from '$lib/ipc/bindings';

const m = vi.hoisted(() => ({
  instancePathStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'ok' }),
  previewInstanceDirName: vi.fn().mockResolvedValue('Preview-Name'),
  renameInstanceDir: vi.fn(),
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
    default_mb: 2048,
    recommended_max_mb: 8192,
    step_mb: 256,
    ram_known: false,
  }),
  setInstanceName: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  openInstanceFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  openModsFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  deleteInstance: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
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

beforeEach(() => vi.clearAllMocks());

// Alpha: vanilla, ACTIVE and selected when the modal opens. Beta: modded, the
// row the menus are opened on.
function renderModal(over: Record<string, unknown> = {}) {
  const a = makeInstance({ id: 'a', name: 'Alpha' });
  const b = makeInstance({ id: 'b', name: 'Beta', loader: 'fabric', loader_version: '0.16.0' });
  return render(ManageInstancesModal, {
    props: {
      open: true,
      instances: [a, b],
      activeInstance: a,
      versions: [version],
      onChanged: () => {},
      ...over,
    },
  });
}

async function openRowMenu(id: string) {
  await fireEvent.contextMenu(screen.getByTestId(`manage-row-${id}`));
  return screen.getByRole('menu');
}

const isCurrent = (id: string) =>
  screen.getByTestId(`manage-row-${id}`).getAttribute('aria-current') === 'true';

describe('ManageInstancesModal — row context menu', () => {
  it('opens on the row under the pointer and names it, without moving the selection', async () => {
    renderModal();
    await screen.findByDisplayValue('Alpha');
    const menu = await openRowMenu('b');
    expect(menu.getAttribute('aria-label')).toBe('Actions for Beta');
    // A right-click is not a click — same as the log-file rows and the sidebar
    // profile dropdown.
    expect(isCurrent('a')).toBe(true);
    expect(isCurrent('b')).toBe(false);
    expect(screen.getByDisplayValue('Alpha')).toBeTruthy();
  });

  it('does not throw the user out of the New-instance form', async () => {
    renderModal();
    await fireEvent.click(screen.getByRole('button', { name: '+ New instance' }));
    expect(screen.getByRole('heading', { name: 'New instance' })).toBeTruthy();
    await openRowMenu('b');
    expect(screen.getByRole('heading', { name: 'New instance' })).toBeTruthy();
  });

  it('Shift+F10 on a focused row opens that row’s menu and leaves the selection alone', async () => {
    renderModal();
    await screen.findByDisplayValue('Alpha');
    const row = screen.getByTestId('manage-row-b');
    row.focus();
    await fireEvent.keyDown(row, { key: 'F10', shiftKey: true });
    expect(screen.getByRole('menu').getAttribute('aria-label')).toBe('Actions for Beta');
    expect(isCurrent('a')).toBe(true);
  });

  it('Rename selects the row — it needs the form — and focuses the name field', async () => {
    renderModal();
    await screen.findByDisplayValue('Alpha');
    await openRowMenu('b');
    await fireEvent.click(screen.getByTestId('manage-ctx-rename'));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByDisplayValue('Beta')));
    expect(isCurrent('b')).toBe(true);
  });

  it('Delete confirms and deletes the ROW, while the detail pane stays where it was', async () => {
    renderModal();
    await screen.findByDisplayValue('Alpha');
    await openRowMenu('b');
    await fireEvent.click(screen.getByTestId('manage-ctx-delete'));

    const dialog = await screen.findByRole('dialog', { name: 'Delete instance?' });
    expect(within(dialog).getByText('Delete Beta?')).toBeTruthy();
    expect(isCurrent('a')).toBe(true);

    await fireEvent.input(within(dialog).getByTestId('instance-delete-confirm-input'), {
      target: { value: 'Delete' },
    });
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(m.deleteInstance).toHaveBeenCalledWith('b'));
    expect(m.deleteInstance).toHaveBeenCalledTimes(1);
    // Alpha was never the target, so its form is still on screen.
    expect(screen.getByDisplayValue('Alpha')).toBeTruthy();
  });

  it('the Delete button still deletes the selected instance', async () => {
    renderModal();
    await screen.findByDisplayValue('Alpha');
    await fireEvent.click(screen.getByRole('button', { name: /Delete instance/ }));
    const dialog = await screen.findByRole('dialog', { name: 'Delete instance?' });
    expect(within(dialog).getByText('Delete Alpha?')).toBeTruthy();
  });

  it('Make active hands the row id to the page and is absent on the active row', async () => {
    const onActivateRequest = vi.fn().mockResolvedValue(null);
    renderModal({ onActivateRequest });
    await openRowMenu('b');
    await fireEvent.click(screen.getByTestId('manage-ctx-make-active'));
    expect(onActivateRequest).toHaveBeenCalledWith('b');
    await openRowMenu('a');
    expect(screen.queryByTestId('manage-ctx-make-active')).toBeNull();
  });

  it('shows a failed activation inside the modal', async () => {
    renderModal({ onActivateRequest: vi.fn().mockResolvedValue('boom') });
    await openRowMenu('b');
    await fireEvent.click(screen.getByTestId('manage-ctx-make-active'));
    expect(await screen.findByText('boom')).toBeTruthy();
  });

  it('offers neither Make active nor Export on a mount that cannot do them', async () => {
    renderModal();
    await openRowMenu('b');
    expect(screen.queryByTestId('manage-ctx-make-active')).toBeNull();
    expect(screen.queryByTestId('manage-ctx-export')).toBeNull();
  });

  it('opens the folders of the row, and offers no mods folder on vanilla', async () => {
    renderModal();
    await openRowMenu('b');
    await fireEvent.click(screen.getByTestId('manage-ctx-open-folder'));
    expect(m.openInstanceFolder).toHaveBeenCalledWith('b');
    await openRowMenu('b');
    await fireEvent.click(screen.getByTestId('manage-ctx-open-mods-folder'));
    expect(m.openModsFolder).toHaveBeenCalledWith('b');
    await openRowMenu('a');
    expect(screen.queryByTestId('manage-ctx-open-mods-folder')).toBeNull();
  });

  it('routes Clone and Export to the page with the row id', async () => {
    const onCloneRequest = vi.fn();
    const onExportRequest = vi.fn();
    renderModal({ onCloneRequest, onExportRequest });
    await openRowMenu('b');
    await fireEvent.click(screen.getByTestId('manage-ctx-clone-instance'));
    expect(onCloneRequest).toHaveBeenCalledWith('b');
    await openRowMenu('b');
    await fireEvent.click(screen.getByTestId('manage-ctx-export'));
    expect(onExportRequest).toHaveBeenCalledWith('b');
  });

  it('disables Clone and Delete, with the reason, only on the running row', async () => {
    renderModal({ isInstanceRunning: (id: string) => id === 'b' });
    let menu = await openRowMenu('b');
    for (const id of ['manage-ctx-clone-instance', 'manage-ctx-delete']) {
      const item = within(menu).getByTestId(id) as HTMLButtonElement;
      expect(item.disabled).toBe(true);
      expect(item.textContent).toContain('Stop the running game first.');
    }
    // Escape must close the menu only — the modal hosting it stays (Menu stops
    // the key), or the next right-click would have nothing to land on.
    await fireEvent.keyDown(menu, { key: 'Escape' });
    menu = await openRowMenu('a');
    expect((within(menu).getByTestId('manage-ctx-delete') as HTMLButtonElement).disabled).toBe(
      false,
    );
  });

  it('says why the only instance cannot be deleted', async () => {
    const only = makeInstance({ id: 'a', name: 'Alpha' });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [only],
        activeInstance: only,
        versions: [version],
        onChanged: () => {},
      },
    });
    const menu = await openRowMenu('a');
    const del = within(menu).getByTestId('manage-ctx-delete') as HTMLButtonElement;
    expect(del.disabled).toBe(true);
    expect(del.textContent).toContain('The launcher needs at least one instance.');
  });
});
