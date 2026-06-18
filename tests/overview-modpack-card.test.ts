import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modpacksCheckUpdates: vi.fn(),
    modpackFetchToTemp: vi.fn(),
    modpackComputeUpdate: vi.fn(),
    modpackApplyUpdate: vi.fn(),
  },
}));
vi.mock('$lib/ipc/format-error', () => ({
  formatError: vi.fn((e: { kind: string }) => `formatted:${e.kind}`),
}));
vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: ((m: unknown) => void) | null = null;
  },
}));

import type { ModpackInstanceUpdate } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { modpackUpdates } from '$lib/modpacks/modpack-updates.svelte';
import ModpackCard from '$lib/overview/ModpackCard.svelte';

const modrinthInst = {
  id: 'i1',
  name: 'ATM9',
  mc_version: '1.20.1',
  loader: 'forge' as const,
  loader_version: '47.2.0',
  max_heap_mb: 4096,
  extra_jvm_args: '',
  created_unix_ms: null,
  ready: true,
  mrpack_name: 'All the Mods 9',
  mrpack_version: '0.2.60',
  mrpack_project_id: 'p1',
  mrpack_source: 'modrinth' as const,
  mrpack_summary: 'Kitchen-sink pack',
  mrpack_version_id: 'v60',
  integrity: null,
  imported_from: null,
};

function okData(data: ModpackInstanceUpdate[]) {
  return { status: 'ok' as const, data };
}

afterEach(() => {
  vi.clearAllMocks();
  modpackUpdates.reset();
});

describe('ModpackCard', () => {
  it('renders pack name, version and source', () => {
    const { getByText } = render(ModpackCard, {
      props: { instance: modrinthInst, onOpenPack: () => {} },
    });
    expect(getByText(/All the Mods 9/)).toBeTruthy();
    expect(getByText(/0\.2\.60/)).toBeTruthy();
    expect(getByText('Modrinth')).toBeTruthy();
  });

  it('reflects an available update from the store without a manual click', async () => {
    vi.mocked(commands.modpacksCheckUpdates).mockResolvedValue(
      okData([
        {
          instance_id: 'i1',
          status: {
            kind: 'update_available',
            entry: {
              id: 'v64',
              name: 'ATM9 0.2.64',
              version_number: '0.2.64',
              game_versions: ['1.20.1'],
              loaders: ['forge'],
              date_published: '',
            },
          },
        },
      ]),
    );
    await modpackUpdates.sweep(['i1'], { force: true });
    const { getByTestId } = render(ModpackCard, {
      props: { instance: modrinthInst, onOpenPack: () => {} },
    });
    expect(getByTestId('modpack-update-available').textContent).toContain('0.2.64');
  });

  it('shows an update chip after clicking check', async () => {
    vi.mocked(commands.modpacksCheckUpdates).mockResolvedValue(
      okData([
        {
          instance_id: 'i1',
          status: {
            kind: 'update_available',
            entry: {
              id: 'v64',
              name: 'ATM9 0.2.64',
              version_number: '0.2.64',
              game_versions: ['1.20.1'],
              loaders: ['forge'],
              date_published: '',
            },
          },
        },
      ]),
    );
    const { getByTestId } = render(ModpackCard, {
      props: { instance: modrinthInst, onOpenPack: () => {} },
    });
    await fireEvent.click(getByTestId('modpack-check-update'));
    await waitFor(() =>
      expect(getByTestId('modpack-update-available').textContent).toContain('0.2.64'),
    );
  });

  it('shows up-to-date when no newer version exists', async () => {
    vi.mocked(commands.modpacksCheckUpdates).mockResolvedValue(
      okData([{ instance_id: 'i1', status: { kind: 'up_to_date' } }]),
    );
    const { getByTestId } = render(ModpackCard, {
      props: { instance: modrinthInst, onOpenPack: () => {} },
    });
    await fireEvent.click(getByTestId('modpack-check-update'));
    await waitFor(() => expect(getByTestId('modpack-up-to-date')).toBeTruthy());
  });

  it('shows an inline error when the check fails', async () => {
    vi.mocked(commands.modpacksCheckUpdates).mockResolvedValue(
      okData([{ instance_id: 'i1', status: { kind: 'check_failed', message: 'offline' } }]),
    );
    const { getByTestId } = render(ModpackCard, {
      props: { instance: modrinthInst, onOpenPack: () => {} },
    });
    await fireEvent.click(getByTestId('modpack-check-update'));
    await waitFor(() => expect(getByTestId('modpack-update-error')).toBeTruthy());
  });

  it('shows a not-checkable note for a CF pack without an API key', async () => {
    const cf = { ...modrinthInst, mrpack_source: 'curseforge' as const };
    vi.mocked(commands.modpacksCheckUpdates).mockResolvedValue(
      okData([
        { instance_id: 'i1', status: { kind: 'not_checkable', reason: 'needs_curseforge_key' } },
      ]),
    );
    const { getByTestId, queryByTestId } = render(ModpackCard, {
      props: { instance: cf, onOpenPack: () => {} },
    });
    await fireEvent.click(getByTestId('modpack-check-update'));
    await waitFor(() => expect(getByTestId('modpack-not-checkable')).toBeTruthy());
    // The check button is available for all sources now (no Modrinth-only gate).
    expect(queryByTestId('modpack-check-update')).not.toBeNull();
  });

  it('shows an Update button when an update is available and starts the flow on click', async () => {
    vi.mocked(commands.modpackFetchToTemp).mockResolvedValue({ status: 'ok', data: '/tmp/p.mrpack' });
    vi.mocked(commands.modpackComputeUpdate).mockResolvedValue({
      status: 'ok',
      data: { added: [], removed: [], updated: [], new_version_number: '0.2.64', version_bump: null },
    });
    vi.mocked(commands.modpacksCheckUpdates).mockResolvedValue(
      okData([
        {
          instance_id: 'i1',
          status: {
            kind: 'update_available',
            entry: {
              id: 'v64',
              name: 'ATM9 0.2.64',
              version_number: '0.2.64',
              game_versions: ['1.20.1'],
              loaders: ['forge'],
              date_published: '',
            },
          },
        },
      ]),
    );
    await modpackUpdates.sweep(['i1'], { force: true });
    const { getByTestId } = render(ModpackCard, {
      props: { instance: modrinthInst, onOpenPack: () => {}, onUpdated: () => {} },
    });
    await fireEvent.click(getByTestId('modpack-update-apply'));
    await waitFor(() =>
      expect(commands.modpackFetchToTemp).toHaveBeenCalledWith('modrinth', 'p1', 'v64'),
    );
  });

  it('does not show an Update button when up to date', async () => {
    vi.mocked(commands.modpacksCheckUpdates).mockResolvedValue(
      okData([{ instance_id: 'i1', status: { kind: 'up_to_date' } }]),
    );
    await modpackUpdates.sweep(['i1'], { force: true });
    const { queryByTestId } = render(ModpackCard, {
      props: { instance: modrinthInst, onOpenPack: () => {}, onUpdated: () => {} },
    });
    expect(queryByTestId('modpack-update-apply')).toBeNull();
  });
});
