// The per-world sub-row under a library pack (spec 2026-09-24 §4 U1): its
// trash deleted a world's file at once, including the ONLY copy of a pack
// that is not in the library. It now opens the this-world confirmation.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const cmd = vi.hoisted(() => ({
  datapacksListLibrary: vi.fn(),
  runningInstances: vi.fn(),
  datapacksWorldEntryKind: vi.fn(),
  datapacksRemoveFromWorld: vi.fn(),
  datapacksSetEnabledInWorld: vi.fn(),
  modsProjects: vi.fn(),
}));
const ev = vi.hoisted(() => ({ spawn: vi.fn(), exit: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({
  commands: cmd,
  events: { processSpawned: { listen: ev.spawn }, processExited: { listen: ev.exit } },
}));

import InstalledDatapacksView from '$lib/mods/InstalledDatapacksView.svelte';

function placement(world: string, state: string | null, level_dat: string | null = 'present') {
  return { world, state, ignored_reason: null, level_dat };
}
function library(placements: unknown[], in_library = true) {
  cmd.datapacksListLibrary.mockResolvedValue({
    status: 'ok',
    data: {
      entries: [
        {
          pack: {
            filename: 'vm.zip',
            sha1: 'a'.repeat(40),
            size_bytes: 1024,
            name: 'VeinMiner',
            source: null,
            project_id: null,
            version_id: null,
            version_number: null,
            installed_at: '2026-09-24T00:00:00Z',
          },
          in_library,
          compat: { kind: 'unknown' },
          placements,
        },
      ],
      worlds: placements.map((p) => ({
        world: (p as { world: string }).world,
        level_dat: (p as { level_dat: string | null }).level_dat,
      })),
    },
  });
}
async function expandedPlacements(): Promise<HTMLElement> {
  render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
  await fireEvent.click(await screen.findByTestId('datapack-row-expand'));
  return screen.findByTestId('datapack-placements');
}

beforeEach(() => {
  cmd.runningInstances.mockResolvedValue([]);
  cmd.datapacksWorldEntryKind.mockResolvedValue({ status: 'ok', data: { kind: 'library_copy' } });
  cmd.datapacksRemoveFromWorld.mockResolvedValue({ status: 'ok', data: null });
  ev.spawn.mockResolvedValue(() => {});
  ev.exit.mockResolvedValue(() => {});
});
afterEach(() => vi.clearAllMocks());

describe('InstalledDatapacksView — a world sub-row', () => {
  it('a library sub-row trash opens the this-world dialog instead of removing', async () => {
    library([placement('W1', 'enabled')]);
    const rows = await expandedPlacements();
    await fireEvent.click(within(rows).getByRole('button', { name: /remove from this world/i }));
    expect(await screen.findByTestId('datapack-remove-dialog')).toBeTruthy();
    await waitFor(() =>
      expect(cmd.datapacksWorldEntryKind).toHaveBeenCalledWith('inst-1', 'W1', 'vm.zip'),
    );
    expect(cmd.datapacksRemoveFromWorld).not.toHaveBeenCalled();
  });

  it("an ignored sub-row's trash asks first too", async () => {
    library([{ ...placement('W1', 'ignored'), ignored_reason: 'zip_extension_not_lowercase' }]);
    cmd.datapacksWorldEntryKind.mockResolvedValue({ status: 'ok', data: { kind: 'own_file' } });
    const rows = await expandedPlacements();
    await fireEvent.click(within(rows).getByRole('button', { name: /remove from this world/i }));
    const dialog = await screen.findByTestId('datapack-remove-dialog');
    expect(await within(dialog).findByText(/deletes it permanently/i)).toBeTruthy();
    expect(cmd.datapacksRemoveFromWorld).not.toHaveBeenCalled();
  });
});
