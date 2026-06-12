import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

// Bundle 2: ImportedView calls modpack_status per pack-instance on
// mount and forwards the resulting is_modified flag to each
// ImportedCard. ImportedDetailDrawer (used by the drawer-open path)
// also pulls modsListInstalled, modsProject, modpackStatus, and
// modpackRestoreFile — mock them all so happy-path tests in this file
// don't crash when the drawer renders.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modpackStatus: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsProject: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modpackRestoreFile: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    deleteInstance: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modpackUpdateStatus: vi.fn().mockResolvedValue({ status: 'ok', data: { kind: 'up_to_date' } }),
    // ImportedView's mount sweep drives the shared modpackUpdates store,
    // which calls this command once per pack-instance batch. Return an
    // empty rows array so the sweep resolves without touching badge state.
    modpacksCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import type { LoaderKind } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import ImportedView from '$lib/modpacks/ImportedView.svelte';
import { modpackUpdates } from '$lib/modpacks/modpack-updates.svelte';

// The filters are now custom <Select> listboxes (a themeable
// button[role=combobox] → ul[role=listbox] of li[role=option]) rather
// than native <select>s. Driving one means opening it (click the
// trigger, found by its data-testid) then committing an option — the
// Select commits on `mousedown`, so we fire mouseDown on the option
// matched by its visible label.
async function pickOption(testid: string, name: RegExp | string) {
  await fireEvent.click(screen.getByTestId(testid));
  await fireEvent.mouseDown(screen.getByRole('option', { name }));
  await tick();
}

// Factory for `InstanceWithStatus` test rows. Fields the view doesn't
// read are filled with sensible defaults; the ones that actually drive
// behaviour are exposed as optional overrides so individual cases stay
// readable.
const inst = (
  id: string,
  mrpackName: string | null,
  ms: number,
  overrides: Partial<{
    name: string;
    mc_version: string;
    loader: LoaderKind;
    loader_version: string | null;
  }> = {},
) =>
  ({
    id,
    name: overrides.name ?? `inst-${id}`,
    mc_version: overrides.mc_version ?? '1.20.1',
    loader: overrides.loader ?? ('fabric' as const),
    loader_version: overrides.loader_version ?? '0.15.7',
    max_heap_mb: 2048,
    extra_jvm_args: '',
    created_unix_ms: ms,
    ready: true,
    mrpack_name: mrpackName,
    mrpack_version: mrpackName ? '1.0' : null,
    mrpack_project_id: null,
    mrpack_source: null,
    mrpack_summary: null,
    mrpack_version_id: null,
    integrity: null,
  }) as const;

describe('ImportedView', () => {
  // The mount sweep drives the shared modpackUpdates store (module state),
  // so clear it between cases to keep update-badge state from leaking.
  afterEach(() => modpackUpdates.reset());

  it('filters out non-modpack instances', () => {
    const { queryAllByTestId } = render(ImportedView, {
      props: {
        instances: [inst('a', null, 1), inst('b', 'Pack B', 2)],
        onPick: () => {},
      },
    });
    expect(queryAllByTestId('imported-card').length).toBe(1);
  });

  it('sorts newest first by created_unix_ms', () => {
    const { getAllByTestId } = render(ImportedView, {
      props: {
        instances: [inst('a', 'A', 1000), inst('b', 'B', 2000), inst('c', 'C', 1500)],
        onPick: () => {},
      },
    });
    const rows = getAllByTestId('imported-card');
    expect(rows[0].textContent).toContain('B');
    expect(rows[1].textContent).toContain('C');
    expect(rows[2].textContent).toContain('A');
  });

  it('shows empty state when no packs', () => {
    const { getByText } = render(ImportedView, {
      props: { instances: [], onPick: () => {} },
    });
    expect(getByText(/No packs imported yet/)).toBeTruthy();
  });

  it('search query filters by mrpack_name substring (case-insensitive)', async () => {
    const { getByTestId, queryAllByTestId } = render(ImportedView, {
      props: {
        instances: [
          inst('a', 'Cobblemon', 1, { name: 'pixelmon-clone' }),
          inst('b', 'Better MC', 2, { name: 'vanilla-plus' }),
          inst('c', 'All The Mods 9', 3, { name: 'atm9' }),
        ],
        onPick: () => {},
      },
    });
    const input = getByTestId('imported-search-input') as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'mc' } });
    const cards = queryAllByTestId('imported-card');
    expect(cards.length).toBe(1);
    expect(cards[0].textContent).toContain('Better MC');
  });

  it('search query also matches instance name', async () => {
    const { getByTestId, queryAllByTestId } = render(ImportedView, {
      props: {
        instances: [
          inst('a', 'Cobblemon', 1, { name: 'pixelmon-clone' }),
          inst('b', 'Better MC', 2, { name: 'vanilla-plus' }),
        ],
        onPick: () => {},
      },
    });
    const input = getByTestId('imported-search-input') as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'pixel' } });
    const cards = queryAllByTestId('imported-card');
    expect(cards.length).toBe(1);
    expect(cards[0].textContent).toContain('Cobblemon');
  });

  it('MC dropdown filters by mc_version', async () => {
    const { queryAllByTestId } = render(ImportedView, {
      props: {
        instances: [
          inst('a', 'A', 1, { mc_version: '1.20.1' }),
          inst('b', 'B', 2, { mc_version: '1.21.1' }),
          inst('c', 'C', 3, { mc_version: '1.21.1' }),
        ],
        onPick: () => {},
      },
    });
    await pickOption('imported-mc-filter', '1.21.1');
    const cards = queryAllByTestId('imported-card');
    expect(cards.length).toBe(2);
    for (const card of cards) {
      expect(card.textContent).toContain('1.21.1');
    }
  });

  it('loader dropdown filters by loader', async () => {
    const { queryAllByTestId } = render(ImportedView, {
      props: {
        instances: [
          inst('a', 'A', 1, { loader: 'fabric' }),
          inst('b', 'B', 2, { loader: 'forge' }),
          inst('c', 'C', 3, { loader: 'neoforge' }),
        ],
        onPick: () => {},
      },
    });
    await pickOption('imported-loader-filter', 'NeoForge');
    const cards = queryAllByTestId('imported-card');
    expect(cards.length).toBe(1);
    expect(cards[0].textContent).toContain('NeoForge');
  });

  it('sort = name produces alphabetical order by mrpack_name', async () => {
    const { getAllByTestId } = render(ImportedView, {
      props: {
        instances: [inst('a', 'Charlie', 3000), inst('b', 'Alpha', 1000), inst('c', 'Bravo', 2000)],
        onPick: () => {},
      },
    });
    await pickOption('imported-sort', 'Name (A–Z)');
    const cards = getAllByTestId('imported-card');
    expect(cards[0].textContent).toContain('Alpha');
    expect(cards[1].textContent).toContain('Bravo');
    expect(cards[2].textContent).toContain('Charlie');
  });

  it('sort = oldest reverses newest-first ordering', async () => {
    const { getAllByTestId } = render(ImportedView, {
      props: {
        instances: [inst('a', 'A', 1000), inst('b', 'B', 2000), inst('c', 'C', 1500)],
        onPick: () => {},
      },
    });
    await pickOption('imported-sort', 'Oldest first');
    const cards = getAllByTestId('imported-card');
    expect(cards[0].textContent).toContain('A');
    expect(cards[1].textContent).toContain('C');
    expect(cards[2].textContent).toContain('B');
  });

  it('shows "No packs match the filter" copy when filter zeros out a non-empty list', async () => {
    const { getByTestId, getByText, queryAllByTestId } = render(ImportedView, {
      props: {
        instances: [inst('a', 'Cobblemon', 1), inst('b', 'Better MC', 2)],
        onPick: () => {},
      },
    });
    const input = getByTestId('imported-search-input') as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'zzz-no-match' } });
    expect(queryAllByTestId('imported-card').length).toBe(0);
    expect(getByText(/No packs match the filter/)).toBeTruthy();
  });

  // Bundle 2 — pack-status fetch + modified-tag forwarding.

  it('calls modpackStatus for each pack-instance on mount', async () => {
    vi.mocked(commands.modpackStatus).mockClear();
    render(ImportedView, {
      props: {
        instances: [
          inst('a', 'Pack A', 1),
          inst('b', null, 2), // not a pack — should NOT trigger modpackStatus
          inst('c', 'Pack C', 3),
        ],
        onPick: () => {},
      },
    });
    await waitFor(() => {
      expect(commands.modpackStatus).toHaveBeenCalledTimes(2);
    });
    const ids = vi.mocked(commands.modpackStatus).mock.calls.map((c) => c[0]);
    expect(ids).toContain('a');
    expect(ids).toContain('c');
    expect(ids).not.toContain('b');
  });

  it('passes isModified=true to ImportedCard when modpackStatus says modified', async () => {
    vi.mocked(commands.modpackStatus).mockResolvedValueOnce({
      status: 'ok',
      data: {
        origin: {
          project_id: null,
          source: 'modrinth',
          project_name: 'Pack A',
          version: '1.0',
          files: [],
        },
        installed_shas: ['x'],
        removed_files: [],
        added_count: 1,
        is_modified: true,
        missing_mods: [],
      },
    });
    const { findByTestId } = render(ImportedView, {
      props: {
        instances: [inst('a', 'Pack A', 1)],
        onPick: () => {},
      },
    });
    const tag = await findByTestId('imported-card-modified-tag');
    expect(tag.textContent).toContain('modified');
  });

  // Cross-tab nav rune: modpacksNav triggers the detail drawer.
  describe('modpacksNav', () => {
    afterEach(async () => {
      const { modpacksNav } = await import('$lib/settings/state.svelte');
      modpacksNav.value = null;
    });

    it('opens the detail drawer for the instance named in modpacksNav', async () => {
      const { modpacksNav } = await import('$lib/settings/state.svelte');
      modpacksNav.value = { openDrawerForInstance: 'i1' };
      render(ImportedView, {
        props: {
          instances: [inst('i1', 'My Pack', 1000)],
          onPick: () => {},
        },
      });
      expect(await screen.findByTestId('imported-detail-drawer')).toBeTruthy();
    });
  });
});
