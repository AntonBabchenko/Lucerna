// ServerDatapacksInstalled — the server pane's rows for entries the game
// ignores (spec §2 N.1/N.6, §0.5 A4/A5), and its load error. The pane is
// mounted directly: ServerAddonsTab, its only host, is stubbed out in
// tests/server-addons-tab.test.ts.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ServerDatapackEntry } from '$lib/ipc/bindings';

const cmd = vi.hoisted(() => ({
  serverListDatapacks: vi.fn(),
  serverCheckDatapackUpdates: vi.fn(),
  serverUpdateDatapackOne: vi.fn(),
  serverSetDatapackEnabled: vi.fn(),
  serverRemoveDatapack: vi.fn(),
  vtInstallToServer: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: cmd }));

import { commands } from '$lib/ipc/bindings';
import ServerDatapacksInstalled from '$lib/servers/datapacks/ServerDatapacksInstalled.svelte';
import { hideTooltip, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';
import { revealTooltip } from './test-utils/reveal-tooltip';

function entry(over: Partial<ServerDatapackEntry> = {}): ServerDatapackEntry {
  return {
    record: {
      filename: 'p.zip',
      sha1: 'a'.repeat(40),
      source: 'modrinth',
      project_id: 'terralith',
      version_id: 'v1',
      name: 'Terralith',
      version_number: '2.5.0',
      enrich_attempted: false,
    },
    state: 'enabled',
    present: true,
    is_folder: false,
    ignored_reason: null,
    ...over,
  };
}
function listing(
  level_dat: 'present' | 'only_old' | 'absent' | null,
  entries: ServerDatapackEntry[],
) {
  cmd.serverListDatapacks.mockResolvedValue({ status: 'ok', data: { level_dat, entries } });
}
const mount = () =>
  render(ServerDatapacksInstalled, { props: { serverId: 's1', mcVersion: '1.21.1' } });

beforeEach(() => {
  cmd.serverRemoveDatapack.mockResolvedValue({ status: 'ok', data: null });
  cmd.serverSetDatapackEnabled.mockResolvedValue({ status: 'ok', data: null });
});
afterEach(() => {
  hideTooltip();
  vi.clearAllMocks();
});

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

describe('ServerDatapacksInstalled — a ghost entry (U3)', () => {
  it('is cleared through a quiet confirm that says Minecraft drops the entry itself', async () => {
    listing('present', [
      entry({
        record: { ...entry().record, filename: 'gone.zip', name: 'Gone', sha1: '' },
        state: 'orphaned',
        present: false,
      }),
    ]);
    mount();
    await fireEvent.click(await screen.findByRole('button', { name: /remove data pack/i }));
    const dialog = await screen.findByRole('dialog');
    expect(
      within(dialog).getByText(/drops the entry by itself the next time the server saves/i),
    ).toBeTruthy();
    await fireEvent.click(within(dialog).getByRole('button', { name: /^clear entry$/i }));
    await waitFor(() => expect(cmd.serverRemoveDatapack).toHaveBeenCalledWith('s1', 'gone.zip'));
  });
});

// A reload that fails after an action must not leave the old rows standing
// with the world's level.dat reason gone: on a never-started world that would
// turn the blocked toggles live. Rows and error never render together.
describe('ServerDatapacksInstalled — a reload that fails', () => {
  it('clears the rows instead of keeping them with their gates lifted', async () => {
    const q = entry({ record: { ...entry().record, filename: 'q.zip', name: 'Q' } });
    listing('absent', [entry(), q]);
    mount();
    const removes = await screen.findAllByTestId('server-datapack-remove');
    cmd.serverListDatapacks.mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: 'runtime/world/datapacks', details: 'locked' },
    });
    await fireEvent.click(removes[0]);
    const dialog = await screen.findByRole('dialog');
    await fireEvent.click(within(dialog).getByRole('button', { name: /^remove data pack$/i }));
    await waitFor(() => expect(screen.getByRole('alert').textContent).toMatch(/locked/));
    expect(screen.queryAllByTestId('server-datapack-toggle')).toHaveLength(0);
  });
});

describe('ServerDatapacksInstalled — the note under the toolbar (U4)', () => {
  it('the note does not claim to match /datapack list', async () => {
    listing('present', []);
    mount();
    expect(await screen.findByText(/built-in and mod-provided/)).toBeTruthy();
    expect(screen.queryByText(/matches what \/datapack list shows/)).toBeNull();
  });
});

describe('ServerDatapacksInstalled — level.dat presence (D2)', () => {
  it('only_old disables toggle, trash, update and VT', async () => {
    listing('only_old', [entry()]);
    cmd.serverCheckDatapackUpdates.mockResolvedValue({
      status: 'ok',
      data: [
        {
          filename: 'p.zip',
          name: 'Terralith',
          state: {
            kind: 'update_available',
            latest: { version_id: 'v2', version_number: '2.6.0' },
          },
        },
      ],
    });
    mount();
    const note = await screen.findByTestId('server-datapacks-level-dat-note');
    expect(note.textContent).toMatch(/only the backup copy level\.dat_old is left/);
    // Checking for updates reads only; it stays available.
    await fireEvent.click(screen.getByTestId('server-datapacks-check-updates'));
    const update = (await screen.findByTestId('server-datapack-update')) as HTMLButtonElement;
    expect(update.disabled).toBe(true);
    for (const id of [
      'server-datapack-toggle',
      'server-datapack-remove',
      'server-datapacks-update-all',
      'server-open-vt-builder',
    ]) {
      expect((screen.getByTestId(id) as HTMLButtonElement).disabled, id).toBe(true);
    }
    revealTooltip(screen.getByTestId('server-datapack-remove').closest('span') as HTMLElement);
    expect(tooltipState.text).toBe('Start the server once to restore level.dat');
  });

  it('a row whose state could not be read offers no live toggle, and says why', async () => {
    listing('present', [entry({ state: null })]);
    mount();
    const toggle = (await screen.findByTestId('server-datapack-toggle')) as HTMLButtonElement;
    expect(toggle.disabled).toBe(true);
    revealTooltip(toggle.closest('span') as HTMLElement);
    expect(tooltipState.text).toBe("Couldn't read whether this pack is on or off");
    await fireEvent.click(toggle);
    expect(cmd.serverSetDatapackEnabled).not.toHaveBeenCalled();
  });

  it('absent disables only the toggle, with the reason', async () => {
    listing('absent', [entry()]);
    mount();
    const note = await screen.findByTestId('server-datapacks-level-dat-note');
    expect(note.textContent).toMatch(/hasn't been created yet/);
    const toggle = (await screen.findByTestId('server-datapack-toggle')) as HTMLButtonElement;
    expect(toggle.disabled).toBe(true);
    expect((screen.getByTestId('server-datapack-remove') as HTMLButtonElement).disabled).toBe(
      false,
    );
    expect((screen.getByTestId('server-open-vt-builder') as HTMLButtonElement).disabled).toBe(
      false,
    );
    revealTooltip(toggle.closest('span') as HTMLElement);
    expect(tooltipState.text).toBe('Start the server once first');
  });
});
