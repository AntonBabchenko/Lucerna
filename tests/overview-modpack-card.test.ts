import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modpackCheckUpdate: vi.fn(),
  },
}));

import { commands } from '$lib/ipc/bindings';
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
};

afterEach(() => vi.clearAllMocks());

describe('ModpackCard', () => {
  it('renders pack name, version and source', () => {
    const { getByText } = render(ModpackCard, {
      props: { instance: modrinthInst, onOpenPack: () => {} },
    });
    expect(getByText(/All the Mods 9/)).toBeTruthy();
    expect(getByText(/0\.2\.60/)).toBeTruthy();
    expect(getByText('Modrinth')).toBeTruthy();
  });

  it('shows an update chip when a newer version is found', async () => {
    vi.mocked(commands.modpackCheckUpdate).mockResolvedValue({
      status: 'ok',
      data: {
        id: 'v64',
        name: 'ATM9 0.2.64',
        version_number: '0.2.64',
        game_versions: ['1.20.1'],
        loaders: ['forge'],
        date_published: '',
      },
    });
    const { getByTestId } = render(ModpackCard, {
      props: { instance: modrinthInst, onOpenPack: () => {} },
    });
    await fireEvent.click(getByTestId('modpack-check-update'));
    await waitFor(() =>
      expect(getByTestId('modpack-update-available').textContent).toContain('0.2.64'),
    );
  });

  it('shows up-to-date when no newer version exists', async () => {
    vi.mocked(commands.modpackCheckUpdate).mockResolvedValue({ status: 'ok', data: null });
    const { getByTestId } = render(ModpackCard, {
      props: { instance: modrinthInst, onOpenPack: () => {} },
    });
    await fireEvent.click(getByTestId('modpack-check-update'));
    await waitFor(() => expect(getByTestId('modpack-up-to-date')).toBeTruthy());
  });

  it('shows an inline error when the check fails', async () => {
    vi.mocked(commands.modpackCheckUpdate).mockResolvedValue({
      status: 'error',
      error: { kind: 'network', message: 'offline' } as never,
    });
    const { getByTestId } = render(ModpackCard, {
      props: { instance: modrinthInst, onOpenPack: () => {} },
    });
    await fireEvent.click(getByTestId('modpack-check-update'));
    await waitFor(() => expect(getByTestId('modpack-update-error')).toBeTruthy());
  });

  it('hides the check button and shows the Modrinth-only note for CF packs', () => {
    const cf = { ...modrinthInst, mrpack_source: 'curseforge' as const };
    const { queryByTestId, getByTestId } = render(ModpackCard, {
      props: { instance: cf, onOpenPack: () => {} },
    });
    expect(queryByTestId('modpack-check-update')).toBeNull();
    expect(getByTestId('modpack-only-modrinth')).toBeTruthy();
  });
});
