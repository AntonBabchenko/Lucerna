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
  events: {
    // No-op listener — WorldsTab subscribes to processExited to auto-
    // reload after MC exit. The mock returns a thenable that resolves
    // to an unlisten function (matches @tauri-apps/api/event shape).
    processExited: {
      listen: vi.fn().mockResolvedValue(() => {}),
    },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

describe('WorldsTab', () => {
  it('renders worlds with size and backup-count badge', async () => {
    const { findByText, container } = render(WorldsTab, {
      props: { instanceId: 'i1', onListChanged: () => {} },
    });
    await waitFor(() => findByText('My World'));
    // Exactly one backup-count badge — the 3-backup world; the 0-backup world has none.
    const badges = container.querySelectorAll('.lucide-package');
    expect(badges.length).toBe(1);
    // The count renders next to the icon inside the badge.
    expect(badges[0].parentElement?.textContent).toContain('3');
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
