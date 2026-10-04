// tests/installed-assets-bulk.test.ts
// The add-ons Installed list (resource packs / shaders) with the shared bulk bar: Update over the
// selected rows with a pending update, Remove that asks first (these kinds have no undo), and the
// badge fix the bulk claims depend on — a re-list after a change keeps the other rows' badges.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstalledAsset, ModVersion } from '$lib/ipc/bindings';
import { assetsChanged } from '$lib/settings/state.svelte';

const {
  assetsList,
  assetUninstall,
  assetsCheckUpdates,
  assetInstall,
  assetUpdateOne,
  modsProjects,
  toasts,
} = vi.hoisted(() => ({
  assetsList: vi.fn(),
  assetUninstall: vi.fn(),
  assetsCheckUpdates: vi.fn(),
  assetInstall: vi.fn(),
  assetUpdateOne: vi.fn(),
  modsProjects: vi.fn(),
  toasts: { pushSuccess: vi.fn(), pushWarning: vi.fn() },
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    assetsList,
    assetUninstall,
    assetsCheckUpdates,
    assetInstall,
    assetUpdateOne,
    modsProjects,
  },
}));
vi.mock('$lib/toasts/toasts.svelte', () => toasts);

import InstalledAssetsView from '$lib/mods/InstalledAssetsView.svelte';

function asset(filename: string, over: Partial<InstalledAsset> = {}): InstalledAsset {
  return {
    kind: 'shader',
    filename,
    sha1: filename,
    source: 'modrinth',
    project_id: `proj-${filename}`,
    version_id: 'ver-1',
    name: filename.replace('.zip', ''),
    version_number: '1.0',
    installed_at: '2026-06-01T00:00:00Z',
    ...over,
  };
}
function version(filename: string): ModVersion {
  return {
    source: 'modrinth',
    project_id: `proj-${filename}`,
    version_id: 'ver-2',
    name: filename,
    version_number: '2.0',
    mc_versions: ['1.20.4'],
    loaders: [],
    primary_file: {
      filename,
      url: 'https://example.test/x.zip',
      sha1: 'b',
      size: 1,
      distribution_allowed: true,
    },
    deps: [],
    published_at: null,
  };
}
const ok = <T>(data: T) => ({ status: 'ok' as const, data });

async function mountWith(list: InstalledAsset[]) {
  assetsList.mockResolvedValue(ok(list));
  render(InstalledAssetsView, { props: { instanceId: 'inst-1', kind: 'shader' } });
  await screen.findByTestId('bulk-select-all');
}

beforeEach(() => {
  vi.clearAllMocks();
  modsProjects.mockResolvedValue(ok([]));
  assetUninstall.mockResolvedValue(ok(null));
  assetUpdateOne.mockResolvedValue(ok(null));
  assetsChanged.value = 0;
});
afterEach(() => {
  assetsChanged.value = 0;
});

describe('InstalledAssetsView — bulk actions', () => {
  it('Select all counts the rows; Remove asks with the names and removes nothing until confirmed', async () => {
    await mountWith([asset('a.zip'), asset('b.zip')]);
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    expect(screen.getByText(/2 selected/)).toBeTruthy();
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', { name: 'Remove' }),
    );
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Remove 2 add-ons?')).toBeTruthy();
    expect(within(dialog).getByText('a')).toBeTruthy();
    expect(assetUninstall).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByTestId('assets-bulk-remove-confirm'));
    await waitFor(() => expect(assetUninstall).toHaveBeenCalledTimes(2));
    expect(assetUninstall).toHaveBeenNthCalledWith(1, 'inst-1', 'shader', 'a.zip');
    expect(assetUninstall).toHaveBeenNthCalledWith(2, 'inst-1', 'shader', 'b.zip');
    expect(toasts.pushSuccess).toHaveBeenCalledWith('Removed 2 of 2');
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('Cancel keeps the selection and removes nothing', async () => {
    await mountWith([asset('a.zip'), asset('b.zip')]);
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', { name: 'Remove' }),
    );
    const dialog = await screen.findByRole('dialog');
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(assetUninstall).not.toHaveBeenCalled();
    expect(screen.getByText(/2 selected/)).toBeTruthy();
  });

  it('Update is off until a selected row has a pending update, then applies to those rows only', async () => {
    assetsCheckUpdates.mockResolvedValue(
      ok([
        {
          filename: 'a.zip',
          name: 'a',
          state: { kind: 'update_available', latest: version('a.zip') },
        },
        { filename: 'b.zip', name: 'b', state: { kind: 'up_to_date' } },
      ]),
    );
    await mountWith([asset('a.zip'), asset('b.zip')]);
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    const bar = screen.getByTestId('bulk-bar');
    const update = () => within(bar).getByRole('button', { name: 'Update' });
    expect(update().hasAttribute('disabled')).toBe(true);
    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
    await waitFor(() => expect(update().hasAttribute('disabled')).toBe(false));
    await fireEvent.click(update());
    await waitFor(() => expect(assetUpdateOne).toHaveBeenCalledTimes(1));
    expect(assetUpdateOne).toHaveBeenCalledWith('inst-1', 'shader', 'a.zip', version('a.zip'));
    expect(toasts.pushSuccess).toHaveBeenCalledWith('Updated 1 of 1');
  });

  it('removing one row keeps the other rows’ update badges (the re-list no longer blanks them)', async () => {
    assetsCheckUpdates.mockResolvedValue(
      ok([
        {
          filename: 'a.zip',
          name: 'a',
          state: { kind: 'update_available', latest: version('a.zip') },
        },
        {
          filename: 'b.zip',
          name: 'b',
          state: { kind: 'update_available', latest: version('b.zip') },
        },
      ]),
    );
    await mountWith([asset('a.zip'), asset('b.zip')]);
    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
    await waitFor(() => expect(screen.getAllByRole('button', { name: 'Update' })).toHaveLength(2));
    // The row's own Remove (instant) — the re-list it triggers used to wipe every badge.
    assetsList.mockResolvedValue(ok([asset('b.zip')]));
    await fireEvent.click(screen.getAllByRole('button', { name: 'Remove' })[0]);
    await waitFor(() => expect(assetUninstall).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getAllByRole('button', { name: 'Update' })).toHaveLength(1));
  });

  it('a pack removed elsewhere (Browse) drops out of «Update all», so it is never reinstalled', async () => {
    assetsCheckUpdates.mockResolvedValue(
      ok([
        {
          filename: 'a.zip',
          name: 'a',
          state: { kind: 'update_available', latest: version('a.zip') },
        },
        {
          filename: 'b.zip',
          name: 'b',
          state: { kind: 'update_available', latest: version('b.zip') },
        },
      ]),
    );
    await mountWith([asset('a.zip'), asset('b.zip')]);
    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
    expect(await screen.findByRole('button', { name: 'Update all (2)' })).toBeTruthy();
    // Browse removed a.zip and bumped the shared signal; this view only re-lists.
    assetsList.mockResolvedValue(ok([asset('b.zip')]));
    assetsChanged.value++;
    expect(await screen.findByRole('button', { name: 'Update all (1)' })).toBeTruthy();
  });

  it('removing every row parks focus on the empty list, not on <body>', async () => {
    await mountWith([asset('a.zip'), asset('b.zip')]);
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', { name: 'Remove' }),
    );
    assetsList.mockResolvedValue(ok([]));
    await fireEvent.click(await screen.findByTestId('assets-bulk-remove-confirm'));
    await waitFor(() => expect(assetUninstall).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByTestId('list-empty')));
  });
});
