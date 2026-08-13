/**
 * The missing-dependency hand-off from the compatibility panel.
 *
 * When `mods_install_missing_required` cannot resolve a dependency it returns
 * `open_search`, and the Add-ons shell flips to Browse with the dep-id as a
 * seed. That hand-off was only half-wired: `ModBrowseView` set its internal
 * `query` and ran the search, but `BrowseFilterBar`'s input had no `value`
 * prop, so the visible box stayed blank. The user saw "nothing found" under an
 * empty field and read it as a broken search rather than an honest miss.
 *
 * Observed live 2026-08-12 on a Forge 1.20.6 instance with the dep-id
 * `forgeconfigapiport`. There was no test covering it — this is that test.
 */
import { render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsSearch: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { hits: [], total: 0, offset: 0, page_size: 20 } }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsInstallWithDeps: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
  events: {
    modInstalled: { listen: vi.fn().mockResolvedValue(() => {}) },
    modUninstalled: { listen: vi.fn().mockResolvedValue(() => {}) },
    modToggle: { listen: vi.fn().mockResolvedValue(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import ModBrowseView from '$lib/mods/ModBrowseView.svelte';

const DEP_ID = 'forgeconfigapiport';

function renderSeeded(onSeedConsumed = () => {}) {
  return render(ModBrowseView, {
    props: {
      source: 'modrinth' as const,
      instanceId: 'i1',
      mcVersion: '1.20.6',
      loader: 'forge' as const,
      seedQuery: DEP_ID,
      onSeedConsumed,
    },
  });
}

describe('missing-dependency hand-off to Browse', () => {
  it('shows the seeded query in the search box', async () => {
    renderSeeded();
    await waitFor(() => {
      const input = screen.getByLabelText(/search/i) as HTMLInputElement;
      expect(input.value).toBe(DEP_ID);
    });
  });

  it('still runs the search for the seeded query', async () => {
    const mod = await import('$lib/ipc/bindings');
    const search = mod.commands.modsSearch as unknown as ReturnType<typeof vi.fn>;
    search.mockClear();
    renderSeeded();
    await waitFor(() => {
      const queries = search.mock.calls.map((c) => (c[0] as { query: string }).query);
      expect(queries).toContain(DEP_ID);
    });
  });

  it('consumes the seed once so a later manual edit is not hijacked', async () => {
    const onSeedConsumed = vi.fn();
    renderSeeded(onSeedConsumed);
    await waitFor(() => expect(onSeedConsumed).toHaveBeenCalledTimes(1));
  });
});
