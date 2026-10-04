// tests/world-datapacks-bulk.test.ts
// The world tab's bulk bar: each action applies to the rows whose own control would do the same
// thing (worldBulkApplies), removal goes through the batch dialog (a ghost is cleared without
// asking the backend), and the world's gate turns every action off with its own reason.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { WorldDatapack } from '$lib/ipc/bindings';

const toasts = vi.hoisted(() => ({
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
  pushInfo: vi.fn(),
}));
vi.mock('$lib/toasts/toasts.svelte', () => toasts);
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    datapacksListForWorld: vi.fn(),
    datapacksInstallFromFile: vi.fn(),
    datapacksAddToWorld: vi.fn().mockResolvedValue({ status: 'ok', data: 'linked' }),
    datapacksRemoveFromWorld: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    datapacksWorldEntryKind: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { kind: 'library_copy' } }),
    datapacksSetEnabledInWorld: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

import { commands } from '$lib/ipc/bindings';
import { worldBulkApplies } from '$lib/worlds/datapack-bulk';
import WorldDatapacks from '$lib/worlds/WorldDatapacks.svelte';

const pack = (filename: string, over: Partial<WorldDatapack> = {}): WorldDatapack => ({
  filename,
  state: 'enabled',
  ignored_reason: null,
  in_library: true,
  compat: { kind: 'compatible' },
  ...over,
});
const five = () => [
  pack('on.zip'),
  pack('off.zip', { state: 'disabled' }),
  pack('new.zip', { state: 'not_added' }),
  pack('bad.zip', { state: 'ignored', ignored_reason: 'zip_without_pack_mcmeta' }),
  pack('gone.zip', { state: 'orphaned' }),
];
async function mountWith(packs: WorldDatapack[], running = false) {
  vi.mocked(commands.datapacksListForWorld).mockResolvedValue({
    status: 'ok',
    data: { level_dat: 'present', packs },
  });
  render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld', running } });
  await screen.findByTestId('bulk-select-all');
}
afterEach(() => vi.clearAllMocks());

describe('worldBulkApplies', () => {
  it('maps each row kind to the actions its own control offers', () => {
    const [on, off, add, bad, ghost] = five();
    expect(worldBulkApplies(on, 'disable')).toBe(true);
    expect(worldBulkApplies(on, 'enable')).toBe(false);
    expect(worldBulkApplies(off, 'enable')).toBe(true);
    expect(worldBulkApplies(add, 'add')).toBe(true);
    expect(worldBulkApplies(add, 'remove')).toBe(false);
    expect(
      worldBulkApplies(
        pack('skip.zip', {
          state: 'not_added',
          compat: { kind: 'wont_load', reason: 'no_pack_mcmeta' },
        }),
        'add',
      ),
    ).toBe(false);
    expect(worldBulkApplies(bad, 'enable')).toBe(false);
    expect(worldBulkApplies(bad, 'remove')).toBe(true);
    expect(worldBulkApplies(ghost, 'remove')).toBe(true);
    expect(worldBulkApplies(ghost, 'disable')).toBe(false);
  });
});

describe('WorldDatapacks — bulk actions', () => {
  it('Enable switches on only the disabled live row; Add adds only the addable one', async () => {
    await mountWith(five());
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    expect(screen.getByText(/5 selected/)).toBeTruthy();
    const bar = screen.getByTestId('bulk-bar');
    await fireEvent.click(within(bar).getByRole('button', { name: 'Enable in this world' }));
    await waitFor(() => expect(commands.datapacksSetEnabledInWorld).toHaveBeenCalledTimes(1));
    expect(commands.datapacksSetEnabledInWorld).toHaveBeenCalledWith(
      'inst-1',
      'MyWorld',
      'off.zip',
      true,
    );
    expect(toasts.pushSuccess).toHaveBeenCalledWith('Enabled in this world 1 of 1');
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', { name: 'Add to this world' }),
    );
    await waitFor(() => expect(commands.datapacksAddToWorld).toHaveBeenCalledTimes(1));
    expect(commands.datapacksAddToWorld).toHaveBeenCalledWith('inst-1', 'MyWorld', 'new.zip');
  });

  it('Remove opens the batch dialog for the live, ignored and ghost rows, asking the kind of all but the ghost', async () => {
    await mountWith(five());
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', {
        name: 'Remove from this world',
      }),
    );
    expect(await screen.findByTestId('datapack-bulk-remove-dialog')).toBeTruthy();
    expect(screen.getByText('Remove 4 data packs from this world?')).toBeTruthy();
    await waitFor(() => expect(commands.datapacksWorldEntryKind).toHaveBeenCalledTimes(3));
    expect(commands.datapacksWorldEntryKind).not.toHaveBeenCalledWith(
      'inst-1',
      'MyWorld',
      'gone.zip',
    );
    expect(commands.datapacksRemoveFromWorld).not.toHaveBeenCalled();
    const confirm = screen.getByTestId('datapack-bulk-remove-confirm') as HTMLButtonElement;
    await waitFor(() => expect(confirm.disabled).toBe(false));
    await fireEvent.click(confirm);
    await waitFor(() => expect(commands.datapacksRemoveFromWorld).toHaveBeenCalledTimes(4));
    expect(commands.datapacksRemoveFromWorld).not.toHaveBeenCalledWith(
      'inst-1',
      'MyWorld',
      'new.zip',
    );
  });

  it('while the instance runs every bar action is off with the running reason', async () => {
    await mountWith(five(), true);
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    const bar = screen.getByTestId('bulk-bar');
    for (const name of [
      'Enable in this world',
      'Disable in this world',
      'Add to this world',
      'Remove from this world',
    ]) {
      const btn = within(bar).getByRole('button', { name });
      expect(btn.hasAttribute('disabled')).toBe(true);
      expect(btn.parentElement?.getAttribute('tabindex')).toBe('0');
    }
  });
});
