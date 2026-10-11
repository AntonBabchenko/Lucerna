// The datapack library holds files hardlinked into worlds; a folder cannot be. The Add-ons folder
// button opens the library, so a folder dropped there is named — not ignored in silence — and a
// library that could not be read says so instead of passing for an empty one.
import { render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const cmd = vi.hoisted(() => ({
  datapacksListLibrary: vi.fn(),
  runningInstances: vi.fn(),
  modsProjects: vi.fn(),
}));
const ev = vi.hoisted(() => ({ spawn: vi.fn(), exit: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({
  commands: cmd,
  events: { processSpawned: { listen: ev.spawn }, processExited: { listen: ev.exit } },
}));

import InstalledDatapacksView from '$lib/mods/InstalledDatapacksView.svelte';

function library(libraryFolders: string[] | null) {
  cmd.datapacksListLibrary.mockResolvedValue({
    status: 'ok',
    data: { entries: [], worlds: [], library_folders: libraryFolders },
  });
}

beforeEach(() => {
  cmd.runningInstances.mockResolvedValue([]);
  ev.spawn.mockResolvedValue(() => {});
  ev.exit.mockResolvedValue(() => {});
});
afterEach(() => vi.clearAllMocks());

describe('InstalledDatapacksView — folders in the library', () => {
  it('names the folders the library cannot use', async () => {
    library(['My Pack', 'Other']);
    render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
    const note = await screen.findByTestId('datapack-library-folders');
    expect(note.textContent).toContain('My Pack, Other');
    expect(screen.queryByTestId('datapack-library-unreadable')).toBeNull();
  });

  it('says nothing when the library holds no folder', async () => {
    library([]);
    render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
    await screen.findByTestId('list-empty');
    expect(screen.queryByTestId('datapack-library-folders')).toBeNull();
    expect(screen.queryByTestId('datapack-library-unreadable')).toBeNull();
  });

  it('says when the library could not be read', async () => {
    library(null);
    render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
    expect(await screen.findByTestId('datapack-library-unreadable')).toBeTruthy();
  });
});
