import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import WorldsTab from '$lib/worlds/WorldsTab.svelte';

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue('C:/maps/Skyblock.zip'),
}));

vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    listWorlds: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // WorldsTab queries these alongside the world list; without them the
    // component rejects and vitest reports unhandled errors even though the
    // assertions still pass.
    listOrphanedBackupWorlds: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listStrandedWorlds: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    recoverStrandedWorld: vi.fn(),
    worldImport: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { folder_name: 'Skyblock', size_bytes: 1024, modified_unix_ms: 1, backup_count: 0 },
    }),
  },
  events: { processExited: { listen: vi.fn().mockResolvedValue(() => {}) } },
}));

describe('WorldsTab import', () => {
  beforeEach(() => vi.clearAllMocks());

  it('imports from a picked .zip via the dropzone and refreshes', async () => {
    const dialog = await import('@tauri-apps/plugin-dialog');
    const bindings = await import('$lib/ipc/bindings');
    const toasts = await import('$lib/toasts/toasts.svelte');
    const onListChanged = vi.fn();

    const { findByTestId } = render(WorldsTab, { props: { instanceId: 'i1', onListChanged } });
    const dropzone = await findByTestId('file-dropzone');
    await fireEvent.click(dropzone);

    await waitFor(() =>
      expect(bindings.commands.worldImport).toHaveBeenCalledWith('i1', 'C:/maps/Skyblock.zip'),
    );
    expect(dialog.open).toHaveBeenCalledWith(
      expect.objectContaining({ filters: [{ name: expect.any(String), extensions: ['zip'] }] }),
    );
    expect(toasts.pushSuccess).toHaveBeenCalled();
    expect(onListChanged).toHaveBeenCalled();
    expect(vi.mocked(bindings.commands.listWorlds).mock.calls.length).toBeGreaterThanOrEqual(2);
  });

  it('imports from a picked folder (directory dialog)', async () => {
    const dialog = await import('@tauri-apps/plugin-dialog');
    vi.mocked(dialog.open).mockResolvedValueOnce('C:/maps/MyWorld');
    const bindings = await import('$lib/ipc/bindings');

    const { findByText } = render(WorldsTab, {
      props: { instanceId: 'i1', onListChanged: () => {} },
    });
    const btn = await findByText('From folder…');
    await fireEvent.click(btn);

    await waitFor(() =>
      expect(bindings.commands.worldImport).toHaveBeenCalledWith('i1', 'C:/maps/MyWorld'),
    );
    expect(dialog.open).toHaveBeenCalledWith(expect.objectContaining({ directory: true }));
  });

  it('imports paths dropped onto the worlds tab', async () => {
    const bindings = await import('$lib/ipc/bindings');
    const { droppedWorld } = await import('$lib/settings/state.svelte');
    render(WorldsTab, { props: { instanceId: 'i1', onListChanged: () => {} } });
    droppedWorld.value = ['C:/maps/Dropped.zip'];
    await waitFor(() =>
      expect(bindings.commands.worldImport).toHaveBeenCalledWith('i1', 'C:/maps/Dropped.zip'),
    );
    expect(droppedWorld.value).toBeNull();
  });
});
