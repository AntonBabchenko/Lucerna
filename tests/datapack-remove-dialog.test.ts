// DatapackRemoveDialog's modes (spec 2026-09-24 §4 U1). `this-world` asks the
// backend what the entry IS before offering the button, because only the
// library's own copy survives a removal. Everything else is the only copy.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DatapackPlacementView } from '$lib/ipc/bindings';
import ru from '../src/lib/i18n/locales/ru.json';

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

import DatapackRemoveDialog from '$lib/mods/DatapackRemoveDialog.svelte';

const thisWorld = () => ({
  instanceId: 'inst-1',
  filename: 'vm.zip',
  packName: 'VeinMiner',
  mode: { kind: 'this-world' as const, world: 'MyWorld' },
  onClose: vi.fn(),
  onRemoved: vi.fn(),
});
const kind = (k: string) =>
  cmd.datapacksWorldEntryKind.mockResolvedValue({ status: 'ok', data: { kind: k } });
const confirmBtn = () => screen.getByTestId('datapack-remove-confirm') as HTMLButtonElement;

afterEach(() => vi.clearAllMocks());

describe('DatapackRemoveDialog — this-world', () => {
  it('on a library copy says it stays in the library and labels the button "Remove from world"', async () => {
    kind('library_copy');
    render(DatapackRemoveDialog, { props: thisWorld() });
    expect(await screen.findByText(/removed from this world only/i)).toBeTruthy();
    expect(screen.getByText(/Remove “VeinMiner” from this world\?/)).toBeTruthy();
    expect(confirmBtn().textContent).toMatch(/Remove from world/);
    expect(cmd.datapacksWorldEntryKind).toHaveBeenCalledWith('inst-1', 'MyWorld', 'vm.zip');
  });

  it('on an own file or own folder names permanent deletion', async () => {
    kind('own_file');
    const a = render(DatapackRemoveDialog, { props: thisWorld() });
    expect(await screen.findByText(/deletes it permanently/i)).toBeTruthy();
    expect(confirmBtn().textContent).toMatch(/Delete/);
    a.unmount();
    kind('own_folder');
    render(DatapackRemoveDialog, { props: thisWorld() });
    expect(await screen.findByText(/the folder and everything in it permanently/i)).toBeTruthy();
  });

  it('keeps Confirm disabled until the entry kind is known', async () => {
    let answer!: (v: unknown) => void;
    cmd.datapacksWorldEntryKind.mockReturnValue(new Promise((r) => (answer = r)));
    render(DatapackRemoveDialog, { props: thisWorld() });
    expect(await screen.findByTestId('datapack-remove-checking')).toBeTruthy();
    expect(confirmBtn().disabled).toBe(true);
    answer({ status: 'ok', data: { kind: 'own_file' } });
    await waitFor(() => expect(confirmBtn().disabled).toBe(false));
  });

  it('when the check fails shows the could-not-check copy and still allows Delete', async () => {
    cmd.datapacksWorldEntryKind.mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: 'x', details: 'denied' },
    });
    cmd.datapacksRemoveFromWorld.mockResolvedValue({ status: 'ok', data: null });
    const p = thisWorld();
    render(DatapackRemoveDialog, { props: p });
    expect(
      await screen.findByText(/couldn't check whether this is the library's copy/i),
    ).toBeTruthy();
    expect(confirmBtn().textContent).toMatch(/Delete/);
    await fireEvent.click(confirmBtn());
    await waitFor(() =>
      expect(cmd.datapacksRemoveFromWorld).toHaveBeenCalledWith('inst-1', 'MyWorld', 'vm.zip'),
    );
    expect(cmd.datapacksRemoveFromWorld).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(p.onRemoved).toHaveBeenCalled());
    expect(p.onClose).toHaveBeenCalled();
    expect(toasts.pushSuccess).toHaveBeenCalled();
  });
});

describe('DatapackRemoveDialog — worlds-only', () => {
  it('no longer claims a library copy was already gone', async () => {
    render(DatapackRemoveDialog, {
      props: {
        instanceId: 'inst-1',
        filename: 'vm.zip',
        packName: 'VeinMiner',
        mode: {
          kind: 'worlds-only' as const,
          placements: [
            {
              world: 'A',
              state: 'enabled' as const,
              ignored_reason: null,
              level_dat: 'present' as const,
            },
          ],
        },
        onClose: () => {},
        onRemoved: () => {},
      },
    });
    expect(await screen.findByText(/permanently/)).toBeTruthy();
    expect(screen.queryByText(/already gone/)).toBeNull();
  });
});

// A placement whose state is unknown because its world could not be read is
// not an ordinary affected world: the cascade reports it Failed and keeps the
// library copy (`LibraryRemoval.removed_from_library`). It is listed apart,
// never dropped and never counted as "used in".
describe('DatapackRemoveDialog — worlds Lucerna could not check', () => {
  const view = (
    world: string,
    state: DatapackPlacementView['state'],
    level_dat: DatapackPlacementView['level_dat'] = 'present',
  ): DatapackPlacementView => ({ world, state, ignored_reason: null, level_dat });
  const libraryProps = (placements: DatapackPlacementView[]) => ({
    instanceId: 'inst-1',
    filename: 'vm.zip',
    packName: 'VeinMiner',
    mode: { kind: 'library' as const, placements },
    onClose: () => {},
    onRemoved: () => {},
  });

  it('lists them apart from the affected worlds and says the library copy stays', async () => {
    render(DatapackRemoveDialog, {
      props: libraryProps([
        view('Fine', 'enabled'),
        view('Locked', null),
        view('Gone', null, null),
      ]),
    });
    const unchecked = await screen.findByTestId('datapack-remove-unchecked');
    expect(within(unchecked).getByText('Locked')).toBeTruthy();
    expect(within(unchecked).getByText('Gone')).toBeTruthy();
    expect(within(unchecked).getByText(/2 worlds couldn't be checked/)).toBeTruthy();
    const affected = screen.getByTestId('datapack-remove-affected');
    expect(within(affected).getByText(/used in 1 world/)).toBeTruthy();
    expect(within(affected).getByText('Fine')).toBeTruthy();
    expect(within(affected).queryByText('Locked')).toBeNull();
    expect(screen.getByText(/the pack stays in the library/)).toBeTruthy();
  });

  it('drops the library-stays note when the cascade is off: no world is touched then', async () => {
    render(DatapackRemoveDialog, { props: libraryProps([view('Locked', null)]) });
    await screen.findByTestId('datapack-remove-unchecked');
    expect(screen.queryByTestId('datapack-remove-affected')).toBeNull();
    await fireEvent.click(screen.getByRole('checkbox'));
    expect(screen.queryByText(/the pack stays in the library/)).toBeNull();
  });

  // `ignored` + `unreadable` is Lucerna failing to read the entry, not the
  // game ignoring it: the cascade cannot compare it either, and fails it.
  it('an entry Lucerna could not read is listed as could-not-check', async () => {
    render(DatapackRemoveDialog, {
      props: libraryProps([
        { ...view('Sealed', 'ignored'), ignored_reason: 'unreadable' },
        { ...view('Loose', 'ignored'), ignored_reason: 'zip_extension_not_lowercase' },
      ]),
    });
    const unchecked = await screen.findByTestId('datapack-remove-unchecked');
    expect(within(unchecked).getByText('Sealed')).toBeTruthy();
    expect(within(unchecked).queryByText('Loose')).toBeNull();
    expect(within(screen.getByTestId('datapack-remove-affected')).getByText('Loose')).toBeTruthy();
  });

  // A placement Lucerna could not check at all (the world's datapacks/ could
  // not be read, or R2 could not tell) is `state: null` + `unreadable`, with
  // the world's real presence. The mark is what sets it apart from a folder
  // with no level file, whose `state: null` is a fact: the cascade fails it.
  it('a could-not-tell placement is unchecked, even in a folder with no level file', async () => {
    render(DatapackRemoveDialog, {
      props: libraryProps([
        { ...view('Husk', null, 'absent'), ignored_reason: 'unreadable' },
        { ...view('Sealed', null), ignored_reason: 'unreadable' },
      ]),
    });
    const unchecked = await screen.findByTestId('datapack-remove-unchecked');
    expect(within(unchecked).getByText('Husk')).toBeTruthy();
    expect(within(unchecked).getByText('Sealed')).toBeTruthy();
    expect(screen.queryByTestId('datapack-remove-affected')).toBeNull();
  });

  it('a folder with neither level file is an affected world, not an unchecked one', async () => {
    render(DatapackRemoveDialog, { props: libraryProps([view('Husk', null, 'absent')]) });
    const affected = await screen.findByTestId('datapack-remove-affected');
    expect(within(affected).getByText('Husk')).toBeTruthy();
    expect(screen.queryByTestId('datapack-remove-unchecked')).toBeNull();
    // The cascade unlinks the file there (§0.5 A3): a mark saying Lucerna
    // leaves the folder alone would be false.
    expect(screen.queryByTestId('datapack-remove-world-blocked')).toBeNull();
  });

  it('worlds-only mode lists them apart and says removal tries them too', async () => {
    render(DatapackRemoveDialog, {
      props: {
        ...libraryProps([view('Fine', 'enabled'), view('Locked', null)]),
        mode: {
          kind: 'worlds-only' as const,
          placements: [view('Fine', 'enabled'), view('Locked', null)],
        },
      },
    });
    const unchecked = await screen.findByTestId('datapack-remove-unchecked');
    expect(within(unchecked).getByText('Locked')).toBeTruthy();
    expect(screen.getByText(/tries these worlds as well/)).toBeTruthy();
    expect(screen.queryByText(/the pack stays in the library/)).toBeNull();
  });
});

describe('DatapackRemoveDialog — worlds without a usable level.dat (D2)', () => {
  it('the affected list names why such a world is left alone', async () => {
    render(DatapackRemoveDialog, {
      props: {
        instanceId: 'inst-1',
        filename: 'vm.zip',
        packName: 'VeinMiner',
        mode: {
          kind: 'worlds-only' as const,
          placements: [
            {
              world: 'Broken',
              state: 'disabled' as const,
              ignored_reason: null,
              level_dat: 'only_old' as const,
            },
            {
              world: 'Fine',
              state: 'enabled' as const,
              ignored_reason: null,
              level_dat: 'present' as const,
            },
          ],
        },
        onClose: () => {},
        onRemoved: () => {},
      },
    });
    const reasons = await screen.findAllByTestId('datapack-remove-world-blocked');
    expect(reasons).toHaveLength(1);
    expect(reasons[0].textContent).toMatch(/restore it from the backup/);
  });

  // The cascade refuses an only-old world (D2) and reports it Failed, which
  // keeps the library copy — said before the click, not only in the toast.
  it('with the cascade on, an only-old world says the library copy stays', async () => {
    render(DatapackRemoveDialog, {
      props: {
        instanceId: 'inst-1',
        filename: 'vm.zip',
        packName: 'VeinMiner',
        mode: {
          kind: 'library' as const,
          placements: [
            {
              world: 'Broken',
              state: 'disabled' as const,
              ignored_reason: null,
              level_dat: 'only_old' as const,
            },
          ],
        },
        onClose: () => {},
        onRemoved: () => {},
      },
    });
    expect(await screen.findByText(/stays in the library until/)).toBeTruthy();
    await fireEvent.click(screen.getByRole('checkbox'));
    expect(screen.queryByText(/stays in the library until/)).toBeNull();
  });
});

// A worlds-only removal goes world by world, and every world writer refuses a
// folder with no level.dat or only level.dat_old (D2). Those are listed apart
// as left unchanged, never promised a deletion and never tried.
describe('DatapackRemoveDialog — worlds-only, worlds Lucerna won’t change', () => {
  const place = (
    world: string,
    state: DatapackPlacementView['state'],
    level_dat: DatapackPlacementView['level_dat'],
  ): DatapackPlacementView => ({ world, state, ignored_reason: null, level_dat });
  const worldsOnly = (placements: DatapackPlacementView[]) => ({
    instanceId: 'inst-1',
    filename: 'vm.zip',
    packName: 'VeinMiner',
    mode: { kind: 'worlds-only' as const, placements },
    onClose: () => {},
    onRemoved: () => {},
  });

  it('lists absent and only-old folders apart, with why, and removes from the rest only', async () => {
    cmd.datapacksRemoveFromWorld.mockResolvedValue({ status: 'ok', data: null });
    render(DatapackRemoveDialog, {
      props: worldsOnly([
        place('Fine', 'enabled', 'present'),
        place('Husk', null, 'absent'),
        place('Old', 'disabled', 'only_old'),
      ]),
    });
    const unchanged = await screen.findByTestId('datapack-remove-unchanged');
    expect(within(unchanged).getByText(/2 worlds won't be changed/)).toBeTruthy();
    expect(within(unchanged).getByText(/Husk/)).toBeTruthy();
    expect(within(unchanged).getByText(/Old/)).toBeTruthy();
    expect(within(unchanged).getByText(/no level\.dat/)).toBeTruthy();
    expect(within(unchanged).getByText(/restore it from the backup/)).toBeTruthy();
    const affected = screen.getByTestId('datapack-remove-affected');
    expect(within(affected).getByText(/used in 1 world/)).toBeTruthy();
    expect(within(affected).queryByText(/Husk/)).toBeNull();
    expect(screen.getByText(/every world Lucerna can change/)).toBeTruthy();

    await fireEvent.click(confirmBtn());
    await waitFor(() => expect(toasts.pushSuccess).toHaveBeenCalledTimes(1));
    expect(cmd.datapacksRemoveFromWorld).toHaveBeenCalledTimes(1);
    expect(cmd.datapacksRemoveFromWorld).toHaveBeenCalledWith('inst-1', 'Fine', 'vm.zip');
  });

  it('offers no removal when every world is one Lucerna won’t change', async () => {
    render(DatapackRemoveDialog, {
      props: worldsOnly([place('Old', 'disabled', 'only_old')]),
    });
    await screen.findByTestId('datapack-remove-unchanged');
    expect(screen.queryByTestId('datapack-remove-affected')).toBeNull();
    expect(confirmBtn().disabled).toBe(true);
  });
});

// A worlds-only row has no library copy: a world that could not be cleaned
// must not be reported as "the library copy was kept".
describe('DatapackRemoveDialog — worlds-only failure toast', () => {
  it('names the worlds it could not clean and claims no library copy', async () => {
    cmd.datapacksRemoveFromWorld
      .mockResolvedValueOnce({ status: 'ok', data: null })
      .mockResolvedValueOnce({
        status: 'error',
        error: { kind: 'io', path: 'B/datapacks', details: 'denied' },
      });
    render(DatapackRemoveDialog, {
      props: {
        instanceId: 'inst-1',
        filename: 'vm.zip',
        packName: 'VeinMiner',
        mode: {
          kind: 'worlds-only' as const,
          placements: [
            {
              world: 'A',
              state: 'enabled' as const,
              ignored_reason: null,
              level_dat: 'present' as const,
            },
            {
              world: 'B',
              state: 'enabled' as const,
              ignored_reason: null,
              level_dat: 'present' as const,
            },
          ],
        },
        onClose: () => {},
        onRemoved: () => {},
      },
    });
    await fireEvent.click(await screen.findByTestId('datapack-remove-confirm'));
    await waitFor(() => expect(toasts.pushWarning).toHaveBeenCalledTimes(1));
    const [title, details] = toasts.pushWarning.mock.calls[0];
    expect(title).toMatch(/1 world could not be cleaned/);
    expect(title).not.toMatch(/library copy/);
    expect(details).toEqual([expect.stringMatching(/^B: /)]);
  });
});

// The RU note under "couldn't check" in worlds-only mode: "Удаление попробует
// и эти миры" read as removing the worlds. It names what is removed, and from
// where.
describe('DatapackRemoveDialog — RU copy', () => {
  it('the worlds-only note says the pack is taken out of those worlds', () => {
    expect(ru.addons.datapacks.remove.uncheckedWorldsOnly).toMatch(
      /убрать датапак и из этих миров/,
    );
  });
});
