// The world picker's two load-bearing behaviours (slice-2 design §7.3):
//   * zero worlds renders the EXPLANATION, not an empty checkbox list — this
//     is the no-worlds case that motivated the whole library screen;
//   * ticking a world where the pack is present-but-DISABLED re-enables it
//     via datapacks_set_enabled_in_world, NEVER datapacks_add_to_world —
//     the add entry point re-materializes the file and force-enables, and
//     routing the toggle through it was called out by the audit as the
//     silent-re-enable defect reachable straight from this picker.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    listWorldNames: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    datapacksAddToWorld: vi.fn().mockResolvedValue({ status: 'ok', data: 'linked' }),
    datapacksSetEnabledInWorld: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
}));

import { commands } from '$lib/ipc/bindings';
import DatapackWorldPicker from '$lib/mods/DatapackWorldPicker.svelte';

const baseProps = {
  instanceId: 'inst',
  filename: 'terralith.zip',
  packName: 'Terralith',
  placements: [],
  worlds: [],
  compat: null,
  onClose: () => {},
  onApplied: () => {},
};

const world = (folder_name: string) => ({ folder_name, modified_unix_ms: null });

describe('DatapackWorldPicker', () => {
  afterEach(() => {
    vi.mocked(commands.listWorldNames).mockResolvedValue({ status: 'ok', data: [] });
    vi.clearAllMocks();
  });

  it('a pack this version skips cannot be ticked (§0.5 I11)', async () => {
    vi.mocked(commands.listWorldNames).mockResolvedValue({ status: 'ok', data: [world('Alpha')] });
    render(DatapackWorldPicker, {
      props: {
        ...baseProps,
        worlds: [{ world: 'Alpha', level_dat: 'present' as const }],
        compat: { kind: 'wont_load' as const, reason: 'no_pack_format' as const },
      },
    });
    const box = (await screen.findByTestId('datapack-picker-world')) as HTMLInputElement;
    expect(box.disabled).toBe(true);
    expect(screen.getByTestId('datapack-picker-wont-load').textContent).toMatch(/skips this pack/);
    expect((screen.getByTestId('datapack-picker-apply') as HTMLButtonElement).disabled).toBe(true);
  });

  it('zero worlds renders the explanation, not an empty list', async () => {
    render(DatapackWorldPicker, { props: baseProps });
    await waitFor(() => {
      expect(screen.getByTestId('datapack-picker-no-worlds')).toBeTruthy();
    });
    expect(screen.queryByTestId('datapack-picker-world')).toBeNull();
    // No apply button either — the only exit is «Library only».
    expect(screen.queryByTestId('datapack-picker-apply')).toBeNull();
  });

  it('ticking a disabled world re-enables it instead of re-adding it', async () => {
    vi.mocked(commands.listWorldNames).mockResolvedValue({
      status: 'ok',
      data: [world('Alpha')],
    });
    render(DatapackWorldPicker, {
      props: {
        ...baseProps,
        placements: [
          {
            world: 'Alpha',
            state: 'disabled' as const,
            level_dat: 'present' as const,
            ignored_reason: null,
          },
        ],
      },
    });
    const box = await screen.findByTestId('datapack-picker-world');
    await fireEvent.click(box);
    await fireEvent.click(screen.getByTestId('datapack-picker-apply'));

    await waitFor(() => {
      expect(vi.mocked(commands.datapacksSetEnabledInWorld)).toHaveBeenCalledWith(
        'inst',
        'Alpha',
        'terralith.zip',
        true,
      );
    });
    expect(vi.mocked(commands.datapacksAddToWorld)).not.toHaveBeenCalled();
  });

  it('ticking a world without the pack adds it', async () => {
    vi.mocked(commands.listWorldNames).mockResolvedValue({
      status: 'ok',
      data: [world('Beta')],
    });
    render(DatapackWorldPicker, {
      props: { ...baseProps, worlds: [{ world: 'Beta', level_dat: 'present' as const }] },
    });
    const box = await screen.findByTestId('datapack-picker-world');
    await fireEvent.click(box);
    await fireEvent.click(screen.getByTestId('datapack-picker-apply'));

    await waitFor(() => {
      expect(vi.mocked(commands.datapacksAddToWorld)).toHaveBeenCalledWith(
        'inst',
        'Beta',
        'terralith.zip',
      );
    });
    expect(vi.mocked(commands.datapacksSetEnabledInWorld)).not.toHaveBeenCalled();
  });

  it('a world already enabled renders checked and locked — nothing to submit', async () => {
    vi.mocked(commands.listWorldNames).mockResolvedValue({
      status: 'ok',
      data: [world('Gamma')],
    });
    render(DatapackWorldPicker, {
      props: {
        ...baseProps,
        placements: [
          {
            world: 'Gamma',
            state: 'enabled' as const,
            level_dat: 'present' as const,
            ignored_reason: null,
          },
        ],
      },
    });
    const box = (await screen.findByTestId('datapack-picker-world')) as HTMLInputElement;
    expect(box.checked).toBe(true);
    expect(box.disabled).toBe(true);
    expect((screen.getByTestId('datapack-picker-apply') as HTMLButtonElement).disabled).toBe(true);
  });

  it("each row carries its world's level.dat presence from the library view", async () => {
    vi.mocked(commands.listWorldNames).mockResolvedValue({
      status: 'ok',
      data: [world('Alpha'), world('Beta'), world('Gamma')],
    });
    render(DatapackWorldPicker, {
      props: {
        ...baseProps,
        worlds: [
          { world: 'Alpha', level_dat: 'present' as const },
          { world: 'Beta', level_dat: 'only_old' as const },
        ],
        // A world only the placements know about (the quick listing missed
        // it) still carries the presence its placement reports.
        placements: [
          {
            world: 'Delta',
            state: 'disabled' as const,
            level_dat: 'absent' as const,
            ignored_reason: null,
          },
        ],
      },
    });
    const boxes = await screen.findAllByTestId('datapack-picker-world');
    const byWorld = Object.fromEntries(
      boxes.map((b) => [b.getAttribute('data-world'), b.getAttribute('data-level-dat')]),
    );
    expect(byWorld).toEqual({
      Alpha: 'present',
      Beta: 'only_old',
      Delta: 'absent',
      Gamma: 'unknown',
    });
  });
  // Fallback Q2: the library listing SAW this folder and could not tell
  // whether it holds a level.dat. A placement's older answer must not
  // overwrite that — "could not tell" stays unknown.
  it('a world the library could not tell stays unknown, whatever its placement says', async () => {
    vi.mocked(commands.listWorldNames).mockResolvedValue({ status: 'ok', data: [world('X')] });
    render(DatapackWorldPicker, {
      props: {
        ...baseProps,
        worlds: [{ world: 'X', level_dat: null }],
        placements: [
          {
            world: 'X',
            state: 'disabled' as const,
            level_dat: 'present' as const,
            ignored_reason: null,
          },
        ],
      },
    });
    const box = await screen.findByTestId('datapack-picker-world');
    expect(box.getAttribute('data-level-dat')).toBe('unknown');
  });

  // Both names come from the same read_dir, so the presence is matched on the
  // exact folder name: on a case-sensitive filesystem 'Alpha' and 'alpha'
  // are two worlds, each with its own level.dat.
  // The placements come from the same saves/ listing, so they too belong to
  // the exact folder name. Were 'Alpha' to pick up 'alpha''s disabled
  // placement, ticking it would send a toggle to a world that never held the
  // pack instead of adding it there.
  it('two folders differing only in case each keep their own presence and placement', async () => {
    vi.mocked(commands.listWorldNames).mockResolvedValue({
      status: 'ok',
      data: [world('Alpha'), world('alpha')],
    });
    render(DatapackWorldPicker, {
      props: {
        ...baseProps,
        worlds: [
          { world: 'Alpha', level_dat: 'present' as const },
          { world: 'alpha', level_dat: 'present' as const },
        ],
        placements: [
          {
            world: 'alpha',
            state: 'disabled' as const,
            level_dat: 'present' as const,
            ignored_reason: null,
          },
        ],
      },
    });
    const boxes = await screen.findAllByTestId('datapack-picker-world');
    expect(boxes.map((b) => b.getAttribute('data-world')).sort()).toEqual(['Alpha', 'alpha']);
    const upper = boxes.find((b) => b.getAttribute('data-world') === 'Alpha');
    if (!upper) throw new Error('no Alpha row');
    await fireEvent.click(upper);
    await fireEvent.click(screen.getByTestId('datapack-picker-apply'));

    await waitFor(() => {
      expect(vi.mocked(commands.datapacksAddToWorld)).toHaveBeenCalledWith(
        'inst',
        'Alpha',
        'terralith.zip',
      );
    });
    expect(vi.mocked(commands.datapacksSetEnabledInWorld)).not.toHaveBeenCalled();
  });

  it('a placement for a world the listing missed keeps its row beside a same-but-for-case world', async () => {
    vi.mocked(commands.listWorldNames).mockResolvedValue({ status: 'ok', data: [world('alpha')] });
    render(DatapackWorldPicker, {
      props: {
        ...baseProps,
        placements: [
          {
            world: 'Alpha',
            state: 'disabled' as const,
            level_dat: 'present' as const,
            ignored_reason: null,
          },
        ],
      },
    });
    const boxes = await screen.findAllByTestId('datapack-picker-world');
    expect(boxes.map((b) => b.getAttribute('data-world')).sort()).toEqual(['Alpha', 'alpha']);
  });

  it('two folders differing only in case each keep their own level.dat presence', async () => {
    vi.mocked(commands.listWorldNames).mockResolvedValue({
      status: 'ok',
      data: [world('Alpha'), world('alpha')],
    });
    render(DatapackWorldPicker, {
      props: {
        ...baseProps,
        worlds: [
          { world: 'Alpha', level_dat: 'present' as const },
          { world: 'alpha', level_dat: 'only_old' as const },
        ],
      },
    });
    const boxes = await screen.findAllByTestId('datapack-picker-world');
    const byWorld = Object.fromEntries(
      boxes.map((b) => [b.getAttribute('data-world'), b.getAttribute('data-level-dat')]),
    );
    expect(byWorld).toEqual({ Alpha: 'present', alpha: 'only_old' });
  });

  it('an ignored placement cannot be ticked', async () => {
    vi.mocked(commands.listWorldNames).mockResolvedValue({ status: 'ok', data: [world('Delta')] });
    render(DatapackWorldPicker, {
      props: {
        ...baseProps,
        worlds: [{ world: 'Delta', level_dat: 'present' as const }],
        placements: [
          {
            world: 'Delta',
            level_dat: 'present' as const,
            state: 'ignored' as const,
            ignored_reason: 'zip_extension_not_lowercase' as const,
          },
        ],
      },
    });
    const box = (await screen.findByTestId('datapack-picker-world')) as HTMLInputElement;
    expect(box.disabled).toBe(true);
    expect(box.checked).toBe(false);
    expect(screen.getByText('Ignored by the game')).toBeTruthy();
  });

  it('a world without level.dat cannot be ticked and says why', async () => {
    vi.mocked(commands.listWorldNames).mockResolvedValue({
      status: 'ok',
      data: [world('Broken'), world('NotAWorld')],
    });
    render(DatapackWorldPicker, {
      props: {
        ...baseProps,
        worlds: [
          { world: 'Broken', level_dat: 'only_old' as const },
          { world: 'NotAWorld', level_dat: 'absent' as const },
        ],
      },
    });
    const boxes = (await screen.findAllByTestId('datapack-picker-world')) as HTMLInputElement[];
    expect(boxes.map((b) => b.disabled)).toEqual([true, true]);
    expect(
      screen.getByText('Open this world in Minecraft and restore it from the backup'),
    ).toBeTruthy();
    expect(screen.getByText('Not a world: this folder has no level.dat')).toBeTruthy();
  });

  // Two only-old worlds became two indistinguishable "(" rows: the reason sat
  // beside the name in the same flex row and squeezed the truncated name to
  // one character. The name keeps the row's width and wraps instead of being
  // cut; the reason wraps on its own line under it. (Layout itself is not
  // computed here — this pins the structure that gives it.)
  it('a long reason sits under the world name and never squeezes it', async () => {
    vi.mocked(commands.listWorldNames).mockResolvedValue({
      status: 'ok',
      data: [world('New World (1)'), world('New World (2)')],
    });
    render(DatapackWorldPicker, {
      props: {
        ...baseProps,
        worlds: [
          { world: 'New World (1)', level_dat: 'only_old' as const },
          { world: 'New World (2)', level_dat: 'only_old' as const },
        ],
      },
    });
    const boxes = await screen.findAllByTestId('datapack-picker-world');
    for (const [i, box] of boxes.entries()) {
      const row = box.closest('label') as HTMLElement;
      const name = within(row).getByText(`New World (${i + 1})`);
      const reason = within(row).getByTestId('datapack-picker-blocked');
      // Not a flex sibling of the name on the row: both live in one column.
      expect(reason.parentElement).not.toBe(row);
      expect(reason.parentElement).toBe(name.parentElement);
      expect(reason.classList.contains('block')).toBe(true);
      // The name wraps rather than being cut to an ellipsis.
      expect(name.classList.contains('truncate')).toBe(false);
      expect(name.classList.contains('break-words')).toBe(true);
    }
  });

  it('a world the library listing did not report is not ticked blind', async () => {
    vi.mocked(commands.listWorldNames).mockResolvedValue({ status: 'ok', data: [world('Newer')] });
    render(DatapackWorldPicker, { props: baseProps });
    const box = (await screen.findByTestId('datapack-picker-world')) as HTMLInputElement;
    expect(box.disabled).toBe(true);
    expect(screen.getByText(/state unknown/i)).toBeTruthy();
  });
});
