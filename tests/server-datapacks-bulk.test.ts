// tests/server-datapacks-bulk.test.ts
// The server data packs pane's bulk bar. A toggle applies only where the row's own toggle would
// render and be live (present, not ignored, level.dat allows it, state known); a ghost is removed
// through the same confirm, which says how many of the rows are leftover entries.
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
const toasts = vi.hoisted(() => ({ pushSuccess: vi.fn(), pushWarning: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({ commands: cmd }));
vi.mock('$lib/toasts/toasts.svelte', () => toasts);

import { canBulkToggle } from '$lib/servers/datapacks/datapack-rows';
import ServerDatapacksInstalled from '$lib/servers/datapacks/ServerDatapacksInstalled.svelte';

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
const named = (filename: string, over: Partial<ServerDatapackEntry> = {}) =>
  entry({ record: { ...entry().record, filename, name: filename }, ...over });

function listing(
  level_dat: 'present' | 'only_old' | 'absent' | null,
  entries: ServerDatapackEntry[],
) {
  cmd.serverListDatapacks.mockResolvedValue({ status: 'ok', data: { level_dat, entries } });
}
const mount = (disabled = false) =>
  render(ServerDatapacksInstalled, { props: { serverId: 's1', mcVersion: '1.21.1', disabled } });

beforeEach(() => {
  cmd.serverRemoveDatapack.mockResolvedValue({ status: 'ok', data: null });
  cmd.serverSetDatapackEnabled.mockResolvedValue({ status: 'ok', data: null });
});
afterEach(() => vi.clearAllMocks());

describe('canBulkToggle', () => {
  it('applies to a present, non-ignored row in the other state, when level.dat allows a toggle', () => {
    expect(canBulkToggle(named('a', { state: 'disabled' }), 'present', true)).toBe(true);
    expect(canBulkToggle(named('a', { state: 'enabled' }), 'present', true)).toBe(false);
    expect(canBulkToggle(named('a', { state: 'enabled' }), 'present', false)).toBe(true);
    expect(
      canBulkToggle(
        named('a', { state: 'ignored', ignored_reason: 'zip_without_pack_mcmeta' }),
        'present',
        true,
      ),
    ).toBe(false);
    expect(canBulkToggle(named('a', { state: 'orphaned', present: false }), 'present', false)).toBe(
      false,
    );
    expect(canBulkToggle(named('a', { state: null }), 'present', true)).toBe(false);
    expect(canBulkToggle(named('a', { state: 'disabled' }), 'absent', true)).toBe(false);
    expect(canBulkToggle(named('a', { state: 'disabled' }), 'only_old', true)).toBe(false);
  });
});

describe('ServerDatapacksInstalled — bulk actions', () => {
  it('Enable switches on only the rows that can take it, by record filename', async () => {
    listing('present', [
      named('on.zip', { state: 'enabled' }),
      named('off.zip', { state: 'disabled' }),
      named('ghost.zip', { state: 'not_added', present: false }),
    ]);
    mount();
    await fireEvent.click(await screen.findByTestId('bulk-select-all'));
    expect(screen.getByText(/3 selected/)).toBeTruthy();
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', { name: 'Enable' }),
    );
    await waitFor(() => expect(cmd.serverSetDatapackEnabled).toHaveBeenCalledTimes(1));
    expect(cmd.serverSetDatapackEnabled).toHaveBeenCalledWith('s1', 'off.zip', true);
    expect(toasts.pushSuccess).toHaveBeenCalledWith('Enabled 1 of 1');
  });

  it('Remove asks, counts the leftover entries apart, and removes every selected row on confirm', async () => {
    listing('present', [named('a.zip'), named('ghost.zip', { state: 'orphaned', present: false })]);
    mount();
    await fireEvent.click(await screen.findByTestId('bulk-select-all'));
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', { name: 'Remove' }),
    );
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText("Remove 2 data packs from this server's world?")).toBeTruthy();
    expect(within(dialog).getByText(/1 of them is only a leftover entry/)).toBeTruthy();
    expect(cmd.serverRemoveDatapack).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByTestId('server-datapacks-bulk-remove-confirm'));
    await waitFor(() => expect(cmd.serverRemoveDatapack).toHaveBeenCalledTimes(2));
    expect(cmd.serverRemoveDatapack).toHaveBeenCalledWith('s1', 'a.zip');
    expect(cmd.serverRemoveDatapack).toHaveBeenCalledWith('s1', 'ghost.zip');
  });

  it('only_old turns every bar action off with the level.dat reason; a running server with its own', async () => {
    listing('only_old', [named('a.zip')]);
    mount();
    await fireEvent.click(await screen.findByTestId('bulk-select-all'));
    const bar = screen.getByTestId('bulk-bar');
    for (const name of ['Enable', 'Disable', 'Update', 'Remove']) {
      const btn = within(bar).getByRole('button', { name });
      expect(btn.hasAttribute('disabled')).toBe(true);
      expect(btn.parentElement?.getAttribute('tabindex')).toBe('0');
    }
  });
});
