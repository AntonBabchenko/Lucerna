// A removal or an add already running when its dialog is torn down (here by
// an instance switch, which closes every data pack dialog) must finish the
// job it started, on the instance it started on, and still report the
// result. The dialog's props point at nothing once the owner has cleared its
// target, so a confirm that re-reads them after an `await` throws half-way,
// or aims the rest of a loop at the instance the user switched to.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DatapackLibraryView, InstalledDatapack } from '$lib/ipc/bindings';

const cmd = vi.hoisted(() => ({
  datapacksListLibrary: vi.fn(),
  runningInstances: vi.fn(),
  listWorldNames: vi.fn(),
  modsProjects: vi.fn(),
  datapacksRemoveFromLibrary: vi.fn(),
  datapacksRemoveFromWorld: vi.fn(),
  datapacksAddToWorld: vi.fn(),
}));
const ev = vi.hoisted(() => ({ spawn: vi.fn(), exit: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({
  commands: cmd,
  events: { processSpawned: { listen: ev.spawn }, processExited: { listen: ev.exit } },
}));

import InstalledDatapacksView from '$lib/mods/InstalledDatapacksView.svelte';
import { dismiss, toastList } from '$lib/toasts/toasts.svelte';

const pack: InstalledDatapack = {
  filename: 'vm.zip',
  sha1: 'a'.repeat(40),
  size_bytes: 1024,
  name: 'VeinMiner',
  source: null,
  project_id: null,
  version_id: null,
  version_number: null,
  installed_at: '2026-09-24T00:00:00Z',
};

function listing(inLibrary: boolean, worlds: string[]): DatapackLibraryView {
  return {
    entries: [
      {
        pack,
        in_library: inLibrary,
        compat: { kind: 'unknown' },
        placements: worlds.map((world) => ({
          world,
          state: 'enabled' as const,
          ignored_reason: null,
          level_dat: 'present' as const,
        })),
      },
    ],
    worlds: [
      { world: 'W1', level_dat: 'present' },
      { world: 'W2', level_dat: 'present' },
    ],
  };
}

type Ok<T> = { status: 'ok'; data: T };

/** A call that stays in flight until the test lets it finish. */
function pending<T>(data: T): { promise: Promise<Ok<T>>; finish: () => void } {
  let finish = (): void => {};
  const promise = new Promise<Ok<T>>((resolve) => {
    finish = () => resolve({ status: 'ok', data });
  });
  return { promise, finish: () => finish() };
}

const toastTitles = () => toastList().map((x) => x.title);

beforeEach(() => {
  cmd.runningInstances.mockResolvedValue([]);
  cmd.listWorldNames.mockResolvedValue({
    status: 'ok',
    data: [
      { folder_name: 'W1', modified_unix_ms: null },
      { folder_name: 'W2', modified_unix_ms: null },
    ],
  });
  ev.spawn.mockResolvedValue(() => {});
  ev.exit.mockResolvedValue(() => {});
});
afterEach(() => {
  for (const x of toastList()) dismiss(x.id);
  vi.clearAllMocks();
});

describe('a data pack change in flight when the instance switches', () => {
  it('a library removal still reports its result', async () => {
    cmd.datapacksListLibrary.mockResolvedValue({ status: 'ok', data: listing(true, ['W1']) });
    const call = pending({
      worlds: [{ kind: 'removed', world: 'W1' }],
      removed_from_library: true,
    });
    cmd.datapacksRemoveFromLibrary.mockReturnValue(call.promise);
    const r = render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
    await fireEvent.click(await screen.findByTestId('datapack-remove-btn'));
    await fireEvent.click(await screen.findByTestId('datapack-remove-confirm'));
    await waitFor(() =>
      expect(cmd.datapacksRemoveFromLibrary).toHaveBeenCalledWith('inst-1', 'vm.zip', true),
    );

    await r.rerender({ instanceId: 'inst-2' });
    await waitFor(() => expect(screen.queryByTestId('datapack-remove-dialog')).toBeNull());
    call.finish();

    await waitFor(() => expect(toastTitles()).toContain('Removed VeinMiner'));
  });

  it('a worlds-only removal finishes every world on the instance it started on', async () => {
    cmd.datapacksListLibrary.mockResolvedValue({
      status: 'ok',
      data: listing(false, ['W1', 'W2']),
    });
    const first = pending(null);
    cmd.datapacksRemoveFromWorld
      .mockReturnValueOnce(first.promise)
      .mockResolvedValue({ status: 'ok', data: null });
    const r = render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
    await fireEvent.click(await screen.findByTestId('datapack-remove-btn'));
    await fireEvent.click(await screen.findByTestId('datapack-remove-confirm'));
    await waitFor(() => expect(cmd.datapacksRemoveFromWorld).toHaveBeenCalledTimes(1));

    await r.rerender({ instanceId: 'inst-2' });
    await waitFor(() => expect(screen.queryByTestId('datapack-remove-dialog')).toBeNull());
    first.finish();

    await waitFor(() => expect(cmd.datapacksRemoveFromWorld).toHaveBeenCalledTimes(2));
    expect(cmd.datapacksRemoveFromWorld).toHaveBeenLastCalledWith('inst-1', 'W2', 'vm.zip');
    await waitFor(() => expect(toastTitles()).toContain('Removed VeinMiner'));
  });

  it('an add to worlds finishes every world on the instance it started on', async () => {
    cmd.datapacksListLibrary.mockResolvedValue({ status: 'ok', data: listing(true, []) });
    const first = pending(null);
    cmd.datapacksAddToWorld
      .mockReturnValueOnce(first.promise)
      .mockResolvedValue({ status: 'ok', data: null });
    const r = render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
    await fireEvent.click(await screen.findByTestId('datapack-add-to-worlds'));
    const boxes = await screen.findAllByTestId('datapack-picker-world');
    for (const box of boxes) await fireEvent.click(box);
    await fireEvent.click(screen.getByTestId('datapack-picker-apply'));
    await waitFor(() => expect(cmd.datapacksAddToWorld).toHaveBeenCalledTimes(1));

    await r.rerender({ instanceId: 'inst-2' });
    await waitFor(() => expect(screen.queryByTestId('datapack-world-picker')).toBeNull());
    first.finish();

    await waitFor(() => expect(cmd.datapacksAddToWorld).toHaveBeenCalledTimes(2));
    expect(cmd.datapacksAddToWorld).toHaveBeenLastCalledWith('inst-1', 'W2', 'vm.zip');
    await waitFor(() => expect(toastTitles()).toContain('Added to 2 worlds'));
  });
});
