// The world detail dialog offers a Datapacks tab only where Minecraft has a
// data-pack system (spec 2026-09-24 §4 U2). WorldsTab supplies the answer
// (tests/worlds-tab.test.ts pins that wiring); this pins what the dialog does.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import WorldDetailDialog from '$lib/worlds/WorldDetailDialog.svelte';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    listBackups: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    backupWorld: vi.fn(),
    deleteBackup: vi.fn(),
    openBackupsFolder: vi.fn(),
    datapacksListForWorld: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { level_dat: 'present', packs: [] } }),
    datapacksListLibrary: vi.fn(),
  },
  events: { processExited: { listen: vi.fn().mockResolvedValue(() => {}) } },
}));

const WORLD = { folder_name: 'Old World', size_bytes: 1, modified_unix_ms: 1, backup_count: 0 };
const props = (datapacksSupported: boolean) => ({
  instanceId: 'inst-1',
  world: WORLD,
  datapacksSupported,
  onClose: () => {},
  onChanged: () => {},
});

afterEach(() => vi.clearAllMocks());

describe('WorldDetailDialog — the pre-1.13 Datapacks gate', () => {
  it('hides the Datapacks tab when datapacksSupported is false', async () => {
    render(WorldDetailDialog, { props: props(false) });
    expect(await screen.findByRole('tab', { name: /backups/i })).toBeTruthy();
    expect(screen.queryByRole('tab', { name: /datapacks/i })).toBeNull();
  });

  it('falls back to Backups if the Datapacks tab disappears while active', async () => {
    const { rerender } = render(WorldDetailDialog, { props: props(true) });
    await fireEvent.click(await screen.findByRole('tab', { name: /datapacks/i }));
    await screen.findByTestId('world-datapacks');
    await rerender(props(false));
    await waitFor(() => expect(screen.queryByTestId('world-datapacks')).toBeNull());
    expect(await screen.findByTestId('backups-create-btn')).toBeTruthy();
  });
});
