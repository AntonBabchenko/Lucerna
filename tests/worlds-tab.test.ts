import { render, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import WorldsTab from '$lib/worlds/WorldsTab.svelte';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    listWorlds: vi.fn().mockResolvedValue({
      status: 'ok',
      data: [
        {
          folder_name: 'My World',
          size_bytes: 1024 * 1024 * 200,
          modified_unix_ms: Date.now() - 1000 * 60 * 60 * 2,
          backup_count: 3,
        },
        {
          folder_name: 'Other World',
          size_bytes: 1024 * 1024 * 50,
          modified_unix_ms: Date.now() - 1000 * 60 * 60 * 24,
          backup_count: 0,
        },
      ],
    }),
  },
}));

describe('WorldsTab', () => {
  it('renders worlds with size and backup-count badge', async () => {
    const { findByText, queryByText } = render(WorldsTab, {
      props: { instanceId: 'i1', onListChanged: () => {} },
    });
    await waitFor(() => findByText('My World'));
    // Backup count badge visible for first world (3 backups).
    expect(queryByText(/📦 3/)).toBeTruthy();
    // Badge hidden for second world (0 backups).
    expect(queryByText(/📦 0/)).toBeFalsy();
  });

  it('shows empty state when no worlds', async () => {
    const mod = await import('$lib/ipc/bindings');
    vi.mocked(mod.commands.listWorlds).mockResolvedValueOnce({ status: 'ok', data: [] });
    const { findByText } = render(WorldsTab, {
      props: { instanceId: 'i1', onListChanged: () => {} },
    });
    await findByText(/No worlds yet/);
  });
});
