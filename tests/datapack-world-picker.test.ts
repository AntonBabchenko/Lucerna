// The world picker's two load-bearing behaviours (slice-2 design §7.3):
//   * zero worlds renders the EXPLANATION, not an empty checkbox list — this
//     is the no-worlds case that motivated the whole library screen;
//   * ticking a world where the pack is present-but-DISABLED re-enables it
//     via datapacks_set_enabled_in_world, NEVER datapacks_add_to_world —
//     the add entry point re-materializes the file and force-enables, and
//     routing the toggle through it was called out by the audit as the
//     silent-re-enable defect reachable straight from this picker.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
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
  onClose: () => {},
  onApplied: () => {},
};

const world = (folder_name: string) => ({ folder_name, modified_unix_ms: null });

describe('DatapackWorldPicker', () => {
  afterEach(() => {
    vi.mocked(commands.listWorldNames).mockResolvedValue({ status: 'ok', data: [] });
    vi.clearAllMocks();
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
        placements: [{ world: 'Alpha', state: 'disabled' as const, level_dat: 'present' as const }],
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
    render(DatapackWorldPicker, { props: baseProps });
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
        placements: [{ world: 'Gamma', state: 'enabled' as const, level_dat: 'present' as const }],
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
        placements: [{ world: 'Delta', state: 'disabled' as const, level_dat: 'absent' as const }],
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
        placements: [{ world: 'X', state: 'disabled' as const, level_dat: 'present' as const }],
      },
    });
    const box = await screen.findByTestId('datapack-picker-world');
    expect(box.getAttribute('data-level-dat')).toBe('unknown');
  });

  // Both names come from the same read_dir, so the presence is matched on the
  // exact folder name: on a case-sensitive filesystem 'Alpha' and 'alpha'
  // are two worlds, each with its own level.dat.
  it('two folders differing only in case each keep their own presence', async () => {
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
});
