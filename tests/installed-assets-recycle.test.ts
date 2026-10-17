// tests/installed-assets-recycle.test.ts
// A pack Lucerna cannot download again — no source: put in the folder by hand (adopted, #505) or
// shipped in a modpack's overrides — goes to the recycle bin when removed, and the user is told
// so; the bulk confirm says which packs go there; «Check for updates» does not claim «up to date»
// about packs it cannot check.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstalledAsset } from '$lib/ipc/bindings';

const { assetsList, assetUninstall, assetsCheckUpdates, modsProjects, toasts } = vi.hoisted(() => ({
  assetsList: vi.fn(),
  assetUninstall: vi.fn(),
  assetsCheckUpdates: vi.fn(),
  modsProjects: vi.fn(),
  toasts: { pushSuccess: vi.fn(), pushWarning: vi.fn() },
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: { assetsList, assetUninstall, assetsCheckUpdates, modsProjects },
}));
vi.mock('$lib/toasts/toasts.svelte', () => toasts);

import InstalledAssetsView from '$lib/mods/InstalledAssetsView.svelte';

const ok = <T>(data: T) => ({ status: 'ok' as const, data });

function downloaded(filename: string): InstalledAsset {
  return {
    kind: 'resource_pack',
    filename,
    sha1: filename,
    source: 'modrinth',
    project_id: `proj-${filename}`,
    version_id: 'ver-1',
    name: filename.replace('.zip', ''),
    version_number: '1.0',
    installed_at: '2026-06-01T00:00:00Z',
  };
}
function byHand(filename: string): InstalledAsset {
  return {
    ...downloaded(filename),
    source: null,
    project_id: null,
    version_id: null,
    version_number: null,
  };
}

async function mountWith(files: InstalledAsset[]) {
  assetsList.mockResolvedValue(ok({ files, folders: [] }));
  render(InstalledAssetsView, { props: { instanceId: 'inst-1', kind: 'resource_pack' } });
  await screen.findByTestId('bulk-select-all');
}

const rowRemove = (i: number) =>
  within(screen.getAllByTestId('asset-row')[i]).getByRole('button', { name: 'Remove' });

beforeEach(() => {
  vi.clearAllMocks();
  modsProjects.mockResolvedValue(ok([]));
});

describe('InstalledAssetsView — where a removed pack goes', () => {
  it('says a pack it could not download again went to the recycle bin', async () => {
    assetUninstall.mockResolvedValue(ok('recycle_bin'));
    await mountWith([byHand('My Texture Edit.zip')]);
    await fireEvent.click(rowRemove(0));
    await waitFor(() =>
      expect(toasts.pushSuccess).toHaveBeenCalledWith('Moved My Texture Edit to the recycle bin'),
    );
  });

  it('says a downloaded pack was removed', async () => {
    assetUninstall.mockResolvedValue(ok('deleted'));
    await mountWith([downloaded('Faithful.zip')]);
    await fireEvent.click(rowRemove(0));
    await waitFor(() => expect(toasts.pushSuccess).toHaveBeenCalledWith('Removed Faithful'));
  });

  it('the bulk confirm names the recycle bin when a pack with no source is selected', async () => {
    await mountWith([downloaded('Faithful.zip'), byHand('Mine.zip')]);
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', { name: 'Remove' }),
    );
    const dialog = await screen.findByRole('dialog');
    expect(
      within(dialog).getByText(/did not come from Modrinth or CurseForge, go to the recycle bin/),
    ).toBeTruthy();
  });

  it('the bulk confirm says nothing about the bin when every selected pack can be downloaded again', async () => {
    await mountWith([downloaded('Faithful.zip'), downloaded('Bare.zip')]);
    await fireEvent.click(screen.getByTestId('bulk-select-all'));
    await fireEvent.click(
      within(screen.getByTestId('bulk-bar')).getByRole('button', { name: 'Remove' }),
    );
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).queryByText(/recycle bin/)).toBeNull();
  });
});

describe('InstalledAssetsView — nothing to check', () => {
  it('Check for updates is off when no pack came from Modrinth or CurseForge', async () => {
    await mountWith([byHand('Mine.zip'), byHand('Other.zip')]);
    const check = screen.getByRole('button', { name: /Check for updates/ });
    expect((check as HTMLButtonElement).disabled).toBe(true);
    expect(assetsCheckUpdates).not.toHaveBeenCalled();
  });

  it('stays on when one pack can be checked', async () => {
    await mountWith([byHand('Mine.zip'), downloaded('Faithful.zip')]);
    const check = screen.getByRole('button', { name: /Check for updates/ });
    expect((check as HTMLButtonElement).disabled).toBe(false);
  });
});
