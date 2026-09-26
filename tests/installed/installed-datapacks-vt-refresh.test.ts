// A Vanilla Tweaks build installs each pack into the library under its own
// name. A pack the library already holds is replaced, and the worlds linked to
// the old copy are refreshed; a world that could not be refreshed stays on the
// old bytes. The catalog install names such worlds, and so does this one. A
// build holds many packs, so each warning names its pack.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const cmd = vi.hoisted(() => ({
  datapacksListLibrary: vi.fn(),
  runningInstances: vi.fn(),
  modsProjects: vi.fn(),
  vtCatalogue: vi.fn(),
  vtInstallToInstance: vi.fn(),
}));
const ev = vi.hoisted(() => ({ spawn: vi.fn(), exit: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({
  commands: cmd,
  events: { processSpawned: { listen: ev.spawn }, processExited: { listen: ev.exit } },
}));

import InstalledDatapacksView from '$lib/mods/InstalledDatapacksView.svelte';
import { dismiss, toastList } from '$lib/toasts/toasts.svelte';

beforeEach(() => {
  cmd.runningInstances.mockResolvedValue([]);
  cmd.datapacksListLibrary.mockResolvedValue({ status: 'ok', data: { entries: [], worlds: [] } });
  cmd.vtCatalogue.mockResolvedValue({
    status: 'ok',
    data: {
      categories: [
        { category: 'survival', packs: [{ name: 'graves', display: 'Graves', version: '2.8.5' }] },
      ],
    },
  });
  ev.spawn.mockResolvedValue(() => {});
  ev.exit.mockResolvedValue(() => {});
});
afterEach(() => {
  for (const toast of toastList()) dismiss(toast.id);
  vi.clearAllMocks();
});

describe('InstalledDatapacksView — a Vanilla Tweaks build', () => {
  it('names each pack, and each world its refresh could not update', async () => {
    cmd.vtInstallToInstance.mockResolvedValue({
      status: 'ok',
      data: {
        outcomes: [
          {
            filename: 'graves v2.8.5.zip',
            name: 'Graves',
            installed: true,
            error: null,
            refreshed: [
              { kind: 'refreshed', world: 'Beta' },
              { kind: 'failed', world: 'Alpha', details: 'locked' },
            ],
          },
          {
            filename: 'coords hud v1.0.0.zip',
            name: 'Coords HUD',
            installed: true,
            error: null,
            refreshed: [{ kind: 'failed', world: 'Alpha', details: 'locked' }],
          },
        ],
      },
    });
    render(InstalledDatapacksView, { props: { instanceId: 'inst-1', mcVersion: '1.21.1' } });
    await fireEvent.click(await screen.findByTestId('open-vt-builder'));
    await fireEvent.click(await screen.findByTestId('vt-pack-survival/graves'));
    await fireEvent.click(screen.getByTestId('vt-build'));

    await waitFor(() => expect(toastList().filter((x) => x.kind === 'warning')).toHaveLength(2));
    const [graves, coords] = toastList().filter((x) => x.kind === 'warning');
    expect(graves.title).toBe(
      "The new version of Graves didn't reach every world — these may still be using the old one:",
    );
    expect(graves.lines).toEqual(['Alpha: locked']);
    expect(coords.title).toMatch(/^The new version of Coords HUD didn't reach every world/);
    for (const w of [graves, coords]) expect(w.title).not.toMatch(/retry/i);
  });
});
