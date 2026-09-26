// A dialog opened on the library screen belongs to the instance it was opened
// for. Switching instance must close it: left open, its Confirm would act on
// the NEW instance with the old instance's pack name and world list.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const cmd = vi.hoisted(() => ({
  datapacksListLibrary: vi.fn(),
  runningInstances: vi.fn(),
  datapacksWorldEntryKind: vi.fn(),
  listWorldNames: vi.fn(),
  modsProjects: vi.fn(),
}));
const ev = vi.hoisted(() => ({ spawn: vi.fn(), exit: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({
  commands: cmd,
  events: { processSpawned: { listen: ev.spawn }, processExited: { listen: ev.exit } },
}));

import InstalledDatapacksView from '$lib/mods/InstalledDatapacksView.svelte';

beforeEach(() => {
  cmd.runningInstances.mockResolvedValue([]);
  cmd.datapacksWorldEntryKind.mockResolvedValue({ status: 'ok', data: { kind: 'library_copy' } });
  cmd.listWorldNames.mockResolvedValue({
    status: 'ok',
    data: [{ folder_name: 'W1', modified_unix_ms: null }],
  });
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
          in_library: true,
          compat: { kind: 'unknown' },
          placements: [
            { world: 'W1', state: 'enabled', ignored_reason: null, level_dat: 'present' },
          ],
        },
      ],
      worlds: [{ world: 'W1', level_dat: 'present' }],
    },
  });
  ev.spawn.mockResolvedValue(() => {});
  ev.exit.mockResolvedValue(() => {});
});
afterEach(() => vi.clearAllMocks());

const openers: [string, () => Promise<void>, string][] = [
  [
    'the library removal',
    async () => {
      await fireEvent.click(await screen.findByTestId('datapack-remove-btn'));
    },
    'datapack-remove-dialog',
  ],
  [
    'the world picker',
    async () => {
      await fireEvent.click(await screen.findByTestId('datapack-add-to-worlds'));
    },
    'datapack-world-picker',
  ],
  [
    'the this-world removal',
    async () => {
      await fireEvent.click(await screen.findByTestId('datapack-row-expand'));
      await fireEvent.click(await screen.findByTestId('datapack-placement-remove'));
    },
    'datapack-remove-dialog',
  ],
];

describe('InstalledDatapacksView — switching instance', () => {
  it.each(openers)('closes %s opened for the previous instance', async (_, open, testid) => {
    const r = render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
    await open();
    expect(await screen.findByTestId(testid)).toBeTruthy();
    await r.rerender({ instanceId: 'inst-2' });
    await waitFor(() => expect(screen.queryByTestId(testid)).toBeNull());
  });
});
