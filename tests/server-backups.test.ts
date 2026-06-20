import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { BackupInfo } from '$lib/ipc/bindings';
import ServerBackupsView from '$lib/servers/ServerBackupsView.svelte';

// Mock bindings — IPC must not fire in unit tests.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {},
  events: {},
}));

// Mock format-error so errors are stable strings.
vi.mock('$lib/ipc/format-error', () => ({
  formatError: (_e: unknown) => 'some error',
}));

// ---------------------------------------------------------------------------
// Hoisted mocks so the factory functions can reference them.
// ---------------------------------------------------------------------------
const mocks = vi.hoisted(() => ({
  backupList: vi.fn(),
  backupCreate: vi.fn(),
  backupRestore: vi.fn(),
  backupDelete: vi.fn(),
  running: vi.fn(() => false as boolean),
}));

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    backupList: (...args: unknown[]) => mocks.backupList(...args),
    backupCreate: (...args: unknown[]) => mocks.backupCreate(...args),
    backupRestore: (...args: unknown[]) => mocks.backupRestore(...args),
    backupDelete: (...args: unknown[]) => mocks.backupDelete(...args),
    running: (_id: string) => mocks.running(),
  },
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeBackup(overrides: Partial<BackupInfo> = {}): BackupInfo {
  return {
    file_name: 'backup-20260619-120000.zip',
    created_unix_ms: Date.now() - 5 * 60 * 1000, // 5 min ago
    size_bytes: 1024 * 512, // 512 KB
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('ServerBackupsView', () => {
  beforeAll(() => locale.set('en'));

  it('calls backupCreate when "Create backup" is clicked', async () => {
    mocks.backupList.mockResolvedValue({ ok: true, list: [] });
    mocks.backupCreate.mockResolvedValue({ ok: true, data: makeBackup() });
    mocks.running.mockReturnValue(false);

    render(ServerBackupsView, { props: { serverId: 'srv-1' } });

    const createBtn = await screen.findByText('Create backup');
    await fireEvent.click(createBtn);

    expect(mocks.backupCreate).toHaveBeenCalledWith('srv-1');
  });

  it('shows a backup row with Restore and Delete buttons', async () => {
    mocks.backupList.mockResolvedValue({ ok: true, list: [makeBackup()] });
    mocks.running.mockReturnValue(false);

    render(ServerBackupsView, { props: { serverId: 'srv-1' } });

    expect(await screen.findByText('backup-20260619-120000.zip')).toBeTruthy();
    expect(screen.getAllByText('Restore').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Delete').length).toBeGreaterThan(0);
  });

  it('disables Restore and shows hint when server is running', async () => {
    mocks.backupList.mockResolvedValue({ ok: true, list: [makeBackup()] });
    mocks.running.mockReturnValue(true);

    render(ServerBackupsView, { props: { serverId: 'srv-1' } });

    // Wait for the list to render
    await screen.findByText('backup-20260619-120000.zip');

    const restoreBtn = screen.getByText('Restore').closest('button') as HTMLButtonElement;
    expect(restoreBtn.disabled).toBe(true);
    expect(screen.getByText('Stop the server before restoring')).toBeTruthy();
  });

  it('Restore (stopped) → confirm → calls backupRestore', async () => {
    const backup = makeBackup();
    mocks.backupList.mockResolvedValue({ ok: true, list: [backup] });
    mocks.backupRestore.mockResolvedValue({ ok: true });
    mocks.running.mockReturnValue(false);

    render(ServerBackupsView, { props: { serverId: 'srv-1' } });

    // Wait for list
    await screen.findByText('backup-20260619-120000.zip');

    const restoreBtn = screen.getByText('Restore').closest('button') as HTMLButtonElement;
    await fireEvent.click(restoreBtn);

    // Confirm dialog should appear
    const confirmBtn = await screen.findByText('Restore', {
      selector: 'button[data-confirm]',
    });
    await fireEvent.click(confirmBtn);

    expect(mocks.backupRestore).toHaveBeenCalledWith('srv-1', backup.file_name);
  });
});
