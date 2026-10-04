// tests/datapack-bulk-remove-dialog.test.ts
// N packs out of one world, or out of the library, through one dialog that keeps the single
// dialog's rules: it asks the backend what each world entry IS before it offers a button; one
// cascade answer for the batch; worlds-only packs are listed apart and always removed from their
// worlds; a world Lucerna won't change is never tried; one notice per run.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DatapackPlacementView } from '$lib/ipc/bindings';

const toasts = vi.hoisted(() => ({
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
  pushInfo: vi.fn(),
}));
vi.mock('$lib/toasts/toasts.svelte', () => toasts);
const cmd = vi.hoisted(() => ({
  datapacksWorldEntryKind: vi.fn(),
  datapacksRemoveFromWorld: vi.fn(),
  datapacksRemoveFromLibrary: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: cmd }));

import DatapackBulkRemoveDialog from '$lib/mods/DatapackBulkRemoveDialog.svelte';
import { isUncheckedPlacement, splitPlacements } from '$lib/mods/datapack-remove-model';

const placement = (
  world: string,
  over: Partial<DatapackPlacementView> = {},
): DatapackPlacementView => ({
  world,
  state: 'enabled',
  ignored_reason: null,
  level_dat: 'present',
  ...over,
});
const confirmBtn = () => screen.getByTestId('datapack-bulk-remove-confirm') as HTMLButtonElement;
afterEach(() => vi.clearAllMocks());

describe('datapack-remove-model', () => {
  it('isUncheckedPlacement: no state in a folder with a level file, or an unreadable mark', () => {
    expect(isUncheckedPlacement(placement('A', { state: null }))).toBe(true);
    expect(isUncheckedPlacement(placement('A', { state: null, level_dat: 'absent' }))).toBe(false);
    expect(isUncheckedPlacement(placement('A', { ignored_reason: 'unreadable' }))).toBe(true);
    expect(isUncheckedPlacement(placement('A'))).toBe(false);
  });

  it('splitPlacements: worlds-only leaves absent/only_old worlds untried; library tries them all', () => {
    const ps = [
      placement('A'),
      placement('B', { level_dat: 'only_old' }),
      placement('C', { state: null }),
    ];
    const wo = splitPlacements('worlds-only', ps);
    expect(wo.unchanged.map((p) => p.world)).toEqual(['B']);
    expect(wo.tried.map((p) => p.world)).toEqual(['A', 'C']);
    expect(wo.affected.map((p) => p.world)).toEqual(['A']);
    expect(wo.unchecked.map((p) => p.world)).toEqual(['C']);
    expect(wo.anyOnlyOld).toBe(true);
    const lib = splitPlacements('library', ps);
    expect(lib.unchanged).toEqual([]);
    expect(lib.tried).toHaveLength(3);
  });
});

describe('DatapackBulkRemoveDialog — this-world', () => {
  const entries = [
    { filename: 'lib.zip', name: 'LibCopy', ghost: false },
    { filename: 'own.zip', name: 'OwnFile', ghost: false },
    { filename: 'gone.zip', name: 'Gone', ghost: true },
  ];
  function kinds(map: Record<string, string>) {
    cmd.datapacksWorldEntryKind.mockImplementation(async (_i: string, _w: string, f: string) => ({
      status: 'ok',
      data: { kind: map[f] },
    }));
  }

  it('asks the kind of every non-ghost entry, states the counts, and labels the button Delete when any is own', async () => {
    kinds({ 'lib.zip': 'library_copy', 'own.zip': 'own_file' });
    cmd.datapacksRemoveFromWorld.mockResolvedValue({ status: 'ok', data: null });
    const onRunning = vi.fn();
    const onRemoved = vi.fn();
    render(DatapackBulkRemoveDialog, {
      props: {
        instanceId: 'inst-1',
        mode: { kind: 'this-world', world: 'MyWorld', entries },
        onRunning,
        onRemoved,
        onClose: vi.fn(),
      },
    });
    expect(screen.getByText('Remove 3 data packs from this world?')).toBeTruthy();
    expect(confirmBtn().disabled).toBe(true);
    const body = await screen.findByTestId('datapack-bulk-remove-body');
    expect(within(body).getByText(/1 is removed from this world only/)).toBeTruthy();
    expect(
      within(body).getByText(/1 isn't the library's copy and is deleted permanently/),
    ).toBeTruthy();
    expect(within(body).getByText(/1 file is already gone/)).toBeTruthy();
    expect(cmd.datapacksWorldEntryKind).toHaveBeenCalledTimes(2);
    expect(cmd.datapacksWorldEntryKind).not.toHaveBeenCalledWith('inst-1', 'MyWorld', 'gone.zip');
    await waitFor(() => expect(confirmBtn().disabled).toBe(false));
    expect(confirmBtn().textContent).toMatch(/Delete/);
    await fireEvent.click(confirmBtn());
    await waitFor(() => expect(cmd.datapacksRemoveFromWorld).toHaveBeenCalledTimes(3));
    expect(cmd.datapacksRemoveFromWorld).toHaveBeenCalledWith('inst-1', 'MyWorld', 'gone.zip');
    expect(onRunning.mock.calls.map((c) => c[0])).toEqual([true, false]);
    expect(onRemoved).toHaveBeenCalledTimes(1);
    expect(toasts.pushSuccess).toHaveBeenCalledWith('Removed from this world 3 of 3');
  });

  it('only library copies: the button says Remove from world; a failed entry is named in one warning', async () => {
    kinds({ 'lib.zip': 'library_copy', 'own.zip': 'library_copy' });
    cmd.datapacksRemoveFromWorld
      .mockResolvedValueOnce({ status: 'ok', data: null })
      .mockResolvedValueOnce({
        status: 'error',
        error: { kind: 'io', path: 'saves/MyWorld/datapacks/own.zip', details: 'locked' },
      });
    render(DatapackBulkRemoveDialog, {
      props: {
        instanceId: 'inst-1',
        mode: { kind: 'this-world', world: 'MyWorld', entries: entries.slice(0, 2) },
        onRemoved: vi.fn(),
        onClose: vi.fn(),
      },
    });
    await waitFor(() => expect(confirmBtn().disabled).toBe(false));
    expect(confirmBtn().textContent).toMatch(/Remove from world/);
    await fireEvent.click(confirmBtn());
    await waitFor(() => expect(toasts.pushWarning).toHaveBeenCalledTimes(1));
    expect(toasts.pushWarning.mock.calls[0][0]).toBe('Removed from this world 1 of 2, 1 failed');
    expect(toasts.pushWarning.mock.calls[0][1]).toEqual([
      { names: 'OwnFile', reason: expect.stringMatching(/locked/) },
    ]);
  });

  it('a verdict that cannot be had is worded as a deletion, never guessed', async () => {
    cmd.datapacksWorldEntryKind.mockRejectedValue(new Error('ipc down'));
    render(DatapackBulkRemoveDialog, {
      props: {
        instanceId: 'inst-1',
        mode: { kind: 'this-world', world: 'MyWorld', entries: entries.slice(0, 1) },
        onRemoved: vi.fn(),
        onClose: vi.fn(),
      },
    });
    const body = await screen.findByTestId('datapack-bulk-remove-body');
    expect(within(body).getByText(/1 couldn't be checked/)).toBeTruthy();
    await waitFor(() => expect(confirmBtn().disabled).toBe(false));
    expect(confirmBtn().textContent).toMatch(/Delete/);
  });
});

describe('DatapackBulkRemoveDialog — library', () => {
  const entries = [
    {
      filename: 'a.zip',
      name: 'Alpha',
      inLibrary: true,
      placements: [placement('W1'), placement('W2', { state: null })],
    },
    { filename: 'b.zip', name: 'Beta', inLibrary: true, placements: [placement('W1')] },
    {
      filename: 'c.zip',
      name: 'Gamma',
      inLibrary: false,
      placements: [placement('W3'), placement('W4', { level_dat: 'only_old' })],
    },
  ];

  it('lists the distinct worlds once, the worlds-only pack apart, and passes the one cascade answer', async () => {
    cmd.datapacksRemoveFromLibrary.mockResolvedValue({
      status: 'ok',
      data: { worlds: [{ kind: 'removed', world: 'W1' }], removed_from_library: true },
    });
    cmd.datapacksRemoveFromWorld.mockResolvedValue({ status: 'ok', data: null });
    render(DatapackBulkRemoveDialog, {
      props: {
        instanceId: 'inst-1',
        mode: { kind: 'library', entries },
        onRemoved: vi.fn(),
        onClose: vi.fn(),
      },
    });
    expect(screen.getByText('Remove 3 data packs?')).toBeTruthy();
    const affected = screen.getByTestId('datapack-bulk-remove-affected');
    expect(within(affected).getByText(/Together they are used in 1 world:/)).toBeTruthy();
    expect(
      within(affected)
        .getAllByRole('listitem')
        .map((li) => li.textContent?.trim()),
    ).toEqual(['W1']);
    expect(
      within(screen.getByTestId('datapack-bulk-remove-unchecked')).getByText(
        /1 world couldn't be checked/,
      ),
    ).toBeTruthy();
    expect(screen.getByTestId('datapack-bulk-remove-worlds-only').textContent).toMatch(
      /1 of them is no longer in the library/,
    );
    const cascade = screen.getByTestId('datapack-bulk-remove-cascade') as HTMLInputElement;
    expect(cascade.checked).toBe(true);
    await fireEvent.click(cascade);
    await fireEvent.click(confirmBtn());
    await waitFor(() => expect(cmd.datapacksRemoveFromLibrary).toHaveBeenCalledTimes(2));
    expect(cmd.datapacksRemoveFromLibrary).toHaveBeenCalledWith('inst-1', 'a.zip', false);
    expect(cmd.datapacksRemoveFromLibrary).toHaveBeenCalledWith('inst-1', 'b.zip', false);
    // The worlds-only pack: per tried world, never the only_old one.
    expect(cmd.datapacksRemoveFromWorld).toHaveBeenCalledTimes(1);
    expect(cmd.datapacksRemoveFromWorld).toHaveBeenCalledWith('inst-1', 'W3', 'c.zip');
    expect(toasts.pushSuccess).toHaveBeenCalledWith('Removed 3 of 3');
  });

  it('a world that could not be cleaned fails its pack, named under the reason; kept copies are listed apart', async () => {
    cmd.datapacksRemoveFromLibrary
      .mockResolvedValueOnce({
        status: 'ok',
        data: {
          worlds: [
            {
              kind: 'failed',
              world: 'W1',
              error: { kind: 'io', path: 'saves/W1', details: 'locked' },
            },
          ],
          removed_from_library: false,
        },
      })
      .mockResolvedValueOnce({
        status: 'ok',
        data: { worlds: [{ kind: 'kept_not_ours', world: 'W1' }], removed_from_library: true },
      });
    render(DatapackBulkRemoveDialog, {
      props: {
        instanceId: 'inst-1',
        mode: { kind: 'library', entries: entries.slice(0, 2) },
        onRemoved: vi.fn(),
        onClose: vi.fn(),
      },
    });
    await fireEvent.click(confirmBtn());
    await waitFor(() => expect(toasts.pushWarning).toHaveBeenCalledTimes(1));
    const [title, lines] = toasts.pushWarning.mock.calls[0];
    expect(title).toBe('Removed 1 of 2, 1 failed');
    expect(lines[0]).toBe('Could not be cleaned:');
    expect(lines[1]).toEqual({ names: 'Alpha: W1', reason: expect.stringMatching(/locked/) });
    expect(lines[2]).toBe("Kept, because the copy there is not the library's:");
    expect(lines[3]).toBe('Beta: W1');
  });
});
