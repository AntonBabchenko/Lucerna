import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { markSeen } from '$lib/onboarding/contextual-tours';
import WorldDetailDialog from '$lib/worlds/WorldDetailDialog.svelte';
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
    listBackups: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // WorldsTab queries these alongside the world list; without them the
    // component rejects and vitest reports unhandled errors even though the
    // assertions still pass.
    listOrphanedBackupWorlds: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listStrandedWorlds: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    recoverStrandedWorld: vi.fn(),
    backupWorld: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { filename: 'b.zip', size_bytes: 10 } }),
    openSavesFolder: vi.fn(),
    openBackupsFolder: vi.fn(),
    // The detail dialog's Datapacks tab mounts WorldDatapacks, which fires
    // datapacksListForWorld on mount (and would on any test that switches
    // tabs, since TabBar activation follows focus). Resolved so it never
    // throws even in tests that never touch that tab.
    datapacksListForWorld: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    datapacksListLibrary: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    datapacksInstallFromFile: vi.fn(),
    datapacksAddToWorld: vi.fn(),
    datapacksRemoveFromWorld: vi.fn(),
    datapacksRemoveFromLibrary: vi.fn(),
    datapacksSetEnabledInWorld: vi.fn(),
  },
  events: {
    processExited: { listen: vi.fn().mockResolvedValue(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

// The worlds ContextualTour mounts a role="dialog" overlay on first visit
// (when worlds exist). Mark it seen so role="dialog" unambiguously targets
// the world-detail dialog under test.
beforeEach(() => markSeen('worlds'));
afterEach(() => vi.clearAllMocks());

describe('WorldsTab', () => {
  it('renders worlds with size and backup-count badge', async () => {
    const { findByText, container } = render(WorldsTab, {
      props: { instanceId: 'i1', onListChanged: () => {} },
    });
    await waitFor(() => findByText('My World'));
    const badges = container.querySelectorAll('.lucide-package');
    expect(badges.length).toBe(1);
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

  it('renders a clickable world row per world', async () => {
    const { findByText, container } = render(WorldsTab, {
      props: { instanceId: 'i1', onListChanged: () => {} },
    });
    await findByText('My World');
    const rows = container.querySelectorAll('[data-testid="world-row"]');
    expect(rows.length).toBe(2);
    expect(rows[0].getAttribute('role')).toBe('button');
    expect(rows[0].getAttribute('tabindex')).toBe('0');
  });

  it('renders a filled green play button per world', async () => {
    const { findByText, container } = render(WorldsTab, {
      props: { instanceId: 'i1', onListChanged: () => {} },
    });
    await findByText('My World');
    expect(container.querySelectorAll('button.btn-success').length).toBe(2);
  });

  it('disables the green play button when quick-play is unavailable', async () => {
    const { findByText, container } = render(WorldsTab, {
      props: {
        instanceId: 'i1',
        onListChanged: () => {},
        quickPlayDisabledReason: 'Quick Play needs Minecraft 1.20+',
      },
    });
    await findByText('My World');
    const playButtons = container.querySelectorAll<HTMLButtonElement>('button.btn-success');
    expect(playButtons.length).toBe(2);
    expect([...playButtons].every((b) => b.disabled)).toBe(true);
  });

  it('renders inline backup + delete icon buttons per world', async () => {
    const { findByText, container } = render(WorldsTab, {
      props: { instanceId: 'i1', onListChanged: () => {} },
    });
    await findByText('My World');
    expect(container.querySelectorAll('[data-testid="world-backup-btn"]').length).toBe(2);
    const deleteBtns = container.querySelectorAll('[data-testid="world-delete-btn"]');
    expect(deleteBtns.length).toBe(2);
    expect(deleteBtns[0].className).toContain('btn-icon-danger');
  });

  it('calls backupWorld when the inline backup icon is clicked', async () => {
    const mod = await import('$lib/ipc/bindings');
    const { findByText, container } = render(WorldsTab, {
      props: { instanceId: 'i1', onListChanged: () => {} },
    });
    await findByText('My World');
    const backupBtn = container.querySelector('[data-testid="world-backup-btn"]')!;
    await fireEvent.click(backupBtn);
    expect(mod.commands.backupWorld).toHaveBeenCalledWith('i1', 'My World');
  });

  it('opens the world-detail dialog when a world row is clicked', async () => {
    const { findByText, container } = render(WorldsTab, {
      props: { instanceId: 'i1', onListChanged: () => {} },
    });
    await findByText('My World');
    const row = container.querySelector('[data-testid="world-row"]')!;
    await fireEvent.click(row);
    expect(await screen.findByTestId('world-detail-dialog')).toBeTruthy();
  });

  it('clicking the play button does NOT open the world-detail dialog (stopPropagation)', async () => {
    const { findByText, container } = render(WorldsTab, {
      props: { instanceId: 'i1', onListChanged: () => {} },
    });
    await findByText('My World');
    const play = container.querySelector('button.btn-success')!;
    await fireEvent.click(play);
    expect(screen.queryByTestId('world-detail-dialog')).toBeNull();
  });

  it('clicking the inline backup icon does NOT open the world-detail dialog (stopPropagation)', async () => {
    const { findByText, container } = render(WorldsTab, {
      props: { instanceId: 'i1', onListChanged: () => {} },
    });
    await findByText('My World');
    const backupBtn = container.querySelector('[data-testid="world-backup-btn"]')!;
    await fireEvent.click(backupBtn);
    expect(screen.queryByTestId('world-detail-dialog')).toBeNull();
  });

  it('clicking the inline delete icon opens the delete dialog, not the world-detail dialog (stopPropagation)', async () => {
    const { findByText, container } = render(WorldsTab, {
      props: { instanceId: 'i1', onListChanged: () => {} },
    });
    await findByText('My World');
    const deleteBtn = container.querySelector('[data-testid="world-delete-btn"]')!;
    await fireEvent.click(deleteBtn);
    // The delete dialog opens; the world-detail dialog must not be present.
    // (A literal-text check like /Backups for/ would pass vacuously here now
    // that the detail dialog's title is just the world name — anchor on the
    // dialog's own testid instead so a real regression is still caught.)
    expect(screen.queryByTestId('world-detail-dialog')).toBeNull();
  });
});

describe('WorldDetailDialog (Backups tab) — header back-up action', () => {
  it('renders a backups-create-btn and calls backupWorld on click', async () => {
    const mod = await import('$lib/ipc/bindings');
    vi.mocked(mod.commands.listBackups).mockResolvedValueOnce({ status: 'ok', data: [] });
    render(WorldDetailDialog, {
      props: {
        instanceId: 'i1',
        world: {
          folder_name: 'My World',
          size_bytes: 1,
          modified_unix_ms: Date.now(),
          backup_count: 0,
        },
        onClose: () => {},
        onChanged: () => {},
      },
    });
    const btn = await screen.findByTestId('backups-create-btn');
    await fireEvent.click(btn);
    expect(mod.commands.backupWorld).toHaveBeenCalledWith('i1', 'My World');
  });
});
