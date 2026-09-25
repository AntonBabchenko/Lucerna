// ServerDatapacksInstalled — the server pane's rows for entries the game
// ignores (spec §2 N.1/N.6, §0.5 A4/A5), and its load error. The pane is
// mounted directly: ServerAddonsTab, its only host, is stubbed out in
// tests/server-addons-tab.test.ts.
import { render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

const { serverListDatapacks } = vi.hoisted(() => ({ serverListDatapacks: vi.fn() }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverListDatapacks,
    serverSetDatapackEnabled: vi.fn(),
    serverRemoveDatapack: vi.fn(),
    serverCheckDatapackUpdates: vi.fn(),
    serverUpdateDatapackOne: vi.fn(),
    vtInstallToServer: vi.fn(),
  },
}));

import { commands } from '$lib/ipc/bindings';
import ServerDatapacksInstalled from '$lib/servers/datapacks/ServerDatapacksInstalled.svelte';

afterEach(() => vi.clearAllMocks());

describe('ServerDatapacksInstalled', () => {
  it('an ignored row shows its reason and no toggle', async () => {
    vi.mocked(commands.serverListDatapacks).mockResolvedValueOnce({
      status: 'ok',
      data: {
        level_dat: 'present',
        entries: [
          {
            record: {
              filename: 'Pack.ZIP',
              sha1: '',
              source: null,
              project_id: null,
              version_id: null,
              name: null,
              version_number: null,
              enrich_attempted: false,
            },
            state: 'ignored',
            ignored_reason: 'zip_extension_not_lowercase',
            present: true,
            is_folder: false,
          },
        ],
      },
    });
    render(ServerDatapacksInstalled, { props: { serverId: 's1', mcVersion: '1.21.1' } });
    await screen.findByText(/ends in lower-case \.zip/i);
    expect(screen.queryByRole('button', { name: /^(Enable|Disable)$/ })).toBeNull();
    expect(screen.getByRole('button', { name: 'Remove data pack' })).toBeTruthy();
  });

  // §0.5 A4: a world whose datapacks folder cannot be read fails the listing,
  // and the pane shows that as its load error, with no rows.
  it('a listing that fails is the pane load error, with no rows', async () => {
    vi.mocked(commands.serverListDatapacks).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: 'C:/srv/world/datapacks', details: 'not a directory' },
    });
    render(ServerDatapacksInstalled, { props: { serverId: 's1', mcVersion: '1.21.1' } });
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('C:/srv/world/datapacks');
    expect(screen.queryByRole('button', { name: 'Remove data pack' })).toBeNull();
    expect(screen.queryByText(/no data packs/i)).toBeNull();
  });
});
