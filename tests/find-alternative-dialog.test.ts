import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

const modsSearch = vi.fn();
const modsVersions = vi.fn();
const modsInstallWithDeps = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsSearch: (...a: unknown[]) => modsSearch(...a),
    modsVersions: (...a: unknown[]) => modsVersions(...a),
    modsInstallWithDeps: (...a: unknown[]) => modsInstallWithDeps(...a),
  },
}));

vi.mock('$lib/toasts/toasts.svelte', () => ({ pushSuccess: vi.fn() }));

import type { ModSource } from '$lib/ipc/bindings';
import FindAlternativeDialog from '$lib/mods/FindAlternativeDialog.svelte';

const hit = (project_id: string, name: string) => ({
  source: 'modrinth',
  project_id,
  slug: name.toLowerCase(),
  name,
  summary: '',
  icon_url: null,
  downloads: 0,
  author: 'someone',
  updated_at: null,
});

afterEach(() => {
  vi.clearAllMocks();
});

type OnInstalled = (chosen: { source: ModSource; projectId: string }) => void | Promise<void>;

function renderDialog(onInstalled?: OnInstalled, onClose: () => void = vi.fn()) {
  return render(FindAlternativeDialog, {
    props: {
      modName: 'Create Train Parts',
      mcVersion: '1.21.1',
      loader: 'neoforge',
      instanceId: 'inst-1',
      curseForgeUrl: 'https://www.curseforge.com/minecraft/mc-mods/create-train-parts',
      onClose,
      onInstalled,
    },
  });
}

describe('FindAlternativeDialog', () => {
  it('auto-searches Modrinth on mount and renders plausible candidates', async () => {
    modsSearch.mockResolvedValue({
      status: 'ok',
      data: { hits: [hit('abc', 'Create Train Parts')], total: 1, offset: 0, page_size: 20 },
    });
    renderDialog();
    await waitFor(() => expect(screen.getByTestId('find-alt-results')).toBeTruthy());
    expect(modsSearch).toHaveBeenCalledWith(
      expect.objectContaining({
        source: 'modrinth',
        query: 'Create Train Parts',
        mc_version: '1.21.1',
        loader: 'neoforge',
      }),
    );
    const results = screen.getByTestId('find-alt-results');
    expect(within(results).getByText('Create Train Parts')).toBeTruthy();
  });

  it('shows the not-found state and no install candidates when only unrelated hits return', async () => {
    // Searching a specific mod ("hexerei") where Modrinth returns loosely
    // related hits by name/author — none of these slugs match "hexerei", so
    // the dialog must offer NO install candidates and show the empty state.
    modsSearch.mockResolvedValue({
      status: 'ok',
      data: {
        hits: [hit('1', 'RSInfinityBooster'), hit('2', 'Minis')],
        total: 2,
        offset: 0,
        page_size: 20,
      },
    });
    render(FindAlternativeDialog, {
      props: {
        modName: 'Hexerei',
        mcVersion: '1.21.1',
        loader: 'neoforge',
        instanceId: 'inst-1',
        curseForgeUrl: 'https://www.curseforge.com/minecraft/mc-mods/hexerei',
        onClose: vi.fn(),
      },
    });
    await waitFor(() => expect(screen.getByTestId('find-alt-empty')).toBeTruthy());
    expect(screen.queryByTestId('find-alt-results')).toBeNull();
    expect(screen.queryByRole('button', { name: /install/i })).toBeNull();
  });

  it('shows the empty state when no hits', async () => {
    modsSearch.mockResolvedValue({
      status: 'ok',
      data: { hits: [], total: 0, offset: 0, page_size: 20 },
    });
    renderDialog();
    await waitFor(() => expect(screen.getByTestId('find-alt-empty')).toBeTruthy());
  });

  it('installs the chosen candidate and calls onInstalled', async () => {
    modsSearch.mockResolvedValue({
      status: 'ok',
      data: { hits: [hit('abc', 'Create Train Parts')], total: 1, offset: 0, page_size: 20 },
    });
    modsVersions.mockResolvedValue({
      status: 'ok',
      data: [{ version_id: 'v1', project_id: 'abc', source: 'modrinth' }],
    });
    modsInstallWithDeps.mockResolvedValue({
      status: 'ok',
      data: { primary_name: 'Create Train Parts', installed_dependencies: [] },
    });
    const onInstalled = vi.fn().mockResolvedValue(undefined);
    const onClose = vi.fn();
    renderDialog(onInstalled, onClose);
    await waitFor(() => expect(screen.getByTestId('find-alt-results')).toBeTruthy());
    const results = screen.getByTestId('find-alt-results');
    await fireEvent.click(within(results).getByRole('button'));
    await waitFor(() => expect(modsInstallWithDeps).toHaveBeenCalled());
    expect(modsInstallWithDeps).toHaveBeenCalledWith(
      'inst-1',
      { source: 'modrinth', project_id: 'abc', version_id: 'v1' },
      [],
    );
    expect(onInstalled).toHaveBeenCalledWith({ source: 'modrinth', projectId: 'abc' });
    await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
  });
});
