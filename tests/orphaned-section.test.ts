// Behavioural coverage for the recovery section: what a user can actually do
// with a world a failed restore left behind.
//
// The intent test next door pins WHERE this section mounts (outside WorldsTab's
// exclusive if-chain, so it survives worlds.length === 0). This one pins what it
// DOES, because the section exists to make unreachable data reachable and
// "renders without throwing" would not prove that.

import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    recoverStrandedWorld: vi.fn(),
    listBackups: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    backupWorld: vi.fn(),
    deleteBackup: vi.fn(),
    restoreBackup: vi.fn(),
    openBackupsFolder: vi.fn(),
    openSavesFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
  events: {
    processExited: { listen: vi.fn().mockResolvedValue(() => {}) },
  },
}));

import OrphanedSection from '$lib/worlds/OrphanedSection.svelte';

const STRANDED = [
  {
    dir_name: '.tmp-restoring-My World-0',
    world_folder: 'My World',
    target_occupied: false,
    kind: 'restore',
  },
];
const ORPHANS = [{ world_folder: 'Gone', backup_count: 2, newest_unix_ms: 1_700_000_000_000 }];

afterEach(() => {
  vi.clearAllMocks();
});

function renderSection(over: Partial<Parameters<typeof render>[1]> = {}) {
  return render(OrphanedSection, {
    props: {
      instanceId: 'inst-1',
      orphans: [],
      stranded: [],
      onChanged: () => {},
      ...(over as Record<string, unknown>),
    },
  });
}

describe('OrphanedSection — nothing to recover', () => {
  it('renders neither section when both lists are empty', () => {
    const { container } = renderSection();
    expect(container.querySelector('[data-testid="stranded-section"]')).toBeNull();
    expect(container.querySelector('[data-testid="orphaned-section"]')).toBeNull();
  });
});

describe('OrphanedSection — a world a rollback could not put back', () => {
  it('names the world, not the dot-directory it is parked in', async () => {
    renderSection({ stranded: STRANDED });
    // The user has no reason to care that it is called .tmp-restoring-My World-0;
    // they care that "My World" is recoverable.
    expect(await screen.findByText(/My World/)).toBeTruthy();
  });

  it('a parked move says "move", never "restore", and offers to put it back', async () => {
    renderSection({
      stranded: [
        {
          dir_name: '.tmp-migrate-moved-Base-0',
          world_folder: 'Base',
          target_occupied: false,
          kind: 'migration',
        },
      ],
    });
    expect(await screen.findByText(/Interrupted move/)).toBeTruthy();
    expect(screen.queryByText(/Interrupted restore/)).toBeNull();
    expect(screen.getByText(/"Base"/)).toBeTruthy();
    expect(screen.getByTestId('stranded-recover-btn')).toBeTruthy();
  });

  it('a parked move whose name is taken explains the clash instead of calling it a leftover', async () => {
    renderSection({
      stranded: [
        {
          dir_name: '.tmp-migrate-moved-Base-0',
          world_folder: 'Base',
          target_occupied: true,
          kind: 'migration',
        },
      ],
    });
    expect(await screen.findByText(/waiting for a free name/)).toBeTruthy();
    expect(screen.queryByText(/Leftover copy from a restore/)).toBeNull();
    expect(screen.queryByTestId('stranded-recover-btn')).toBeNull();
  });

  it('recovering calls the command with the directory name and reports the change', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.recoverStrandedWorld).mockResolvedValueOnce({
      status: 'ok',
      data: 'My World',
    });
    const onChanged = vi.fn();
    renderSection({ stranded: STRANDED, onChanged });

    await fireEvent.click(await screen.findByTestId('stranded-recover-btn'));

    expect(commands.recoverStrandedWorld).toHaveBeenCalledWith(
      'inst-1',
      '.tmp-restoring-My World-0',
    );
    // onChanged is what makes the world list refresh — without it the row would
    // sit there after a successful recovery.
    await vi.waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it('surfaces a refusal instead of failing silently', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.recoverStrandedWorld).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'world_recover_target_occupied', world_folder: 'My World' },
    });
    const onChanged = vi.fn();
    renderSection({ stranded: STRANDED, onChanged });

    await fireEvent.click(await screen.findByTestId('stranded-recover-btn'));

    const alert = await screen.findByRole('alert');
    expect(alert.textContent ?? '').not.toBe('');
    expect(onChanged).not.toHaveBeenCalled();
  });
});

describe('OrphanedSection — a leftover from a restore that finished', () => {
  const LEFTOVER = [
    { dir_name: '.tmp-restoring-W-0', world_folder: 'W', target_occupied: true, kind: 'restore' },
  ];

  it('offers no way to put it back, because saves/W is the restored world', async () => {
    renderSection({ stranded: LEFTOVER });

    // The dangerous version of this feature says "your restore didn't finish,
    // put the world back" about a restore that finished — and the refusal the
    // user then hits tells them to delete the world they just restored.
    expect(await screen.findByTestId('leftover-open-folder-btn')).toBeTruthy();
    expect(screen.queryByTestId('stranded-recover-btn')).toBeNull();
    expect(screen.queryByTestId('stranded-section')).toBeNull();
  });
});

describe('OrphanedSection — a backup set whose world is gone', () => {
  it('lists the set and opens its backups', async () => {
    renderSection({ orphans: ORPHANS });
    expect(await screen.findByText('Gone')).toBeTruthy();

    await fireEvent.click(await screen.findByTestId('orphaned-open-btn'));

    // The panel is reused as-is; the locked-mode note replaces the radio group,
    // because Replace calls world_dir_at and would fail WorldNotFound for a
    // world that no longer exists.
    expect(await screen.findByRole('dialog')).toBeTruthy();
  });
});
