import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ModpackHit } from '$lib/ipc/bindings';

// modpack_get_versions resolves to an empty list; the tests here drive
// the distribution-disabled branch, which renders before any version.
const { mockGetVersions, mockFetchToTemp } = vi.hoisted(() => ({
  mockGetVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  mockFetchToTemp: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: { modpackGetVersions: mockGetVersions, modpackFetchToTemp: mockFetchToTemp },
  events: {},
}));

import ModpackVersionDrawer from '$lib/modpacks/ModpackVersionDrawer.svelte';

const baseHit: ModpackHit = {
  project_id: 'p',
  slug: 'rlcraft',
  title: 'RLCraft',
  description: 'desc',
  icon_url: null,
  downloads: 1,
  latest_mc_version: null,
  supported_loaders: [],
  source: 'curseforge',
  distribution_allowed: null,
};

describe('ModpackVersionDrawer', () => {
  beforeEach(() => {
    mockGetVersions.mockClear();
    mockFetchToTemp.mockClear();
  });

  it('shows the CurseForge fallback for a distribution-disabled pack', async () => {
    const hit: ModpackHit = { ...baseHit, distribution_allowed: false };
    const { findByText } = render(ModpackVersionDrawer, {
      props: { hit, onClose: () => {}, onInstall: () => {} },
    });
    expect(await findByText('Open on CurseForge ↗')).toBeTruthy();
  });

  it('lists versions normally for an allowed pack', async () => {
    const hit: ModpackHit = { ...baseHit, distribution_allowed: null };
    render(ModpackVersionDrawer, {
      props: { hit, onClose: () => {}, onInstall: () => {} },
    });
    await waitFor(() => expect(mockGetVersions).toHaveBeenCalledWith('curseforge', 'p'));
  });

  it('shows the fallback when install hits a distribution-disabled file', async () => {
    mockGetVersions.mockResolvedValueOnce({
      status: 'ok',
      data: [
        {
          id: 'v1',
          name: 'RLCraft 2.9',
          version_number: 'rl.zip',
          game_versions: ['1.12.2'],
          loaders: [],
          date_published: '2026-01-01T00:00:00Z',
        },
      ],
    });
    mockFetchToTemp.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'modpack_cf_distribution_disabled', pack_name: 'RLCraft' },
    });
    const hit: ModpackHit = { ...baseHit, distribution_allowed: null };
    const { findByText } = render(ModpackVersionDrawer, {
      props: { hit, onClose: () => {}, onInstall: () => {} },
    });
    const installBtn = await findByText('Install');
    await fireEvent.click(installBtn);
    expect(await findByText('Open on CurseForge ↗')).toBeTruthy();
  });
});
