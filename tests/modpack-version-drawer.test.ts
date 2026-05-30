import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ModpackHit } from '$lib/ipc/bindings';

const { mockGetVersions, mockFetchToTemp, mockProject } = vi.hoisted(() => ({
  mockGetVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  mockFetchToTemp: vi.fn(),
  mockProject: vi.fn().mockResolvedValue({
    status: 'ok',
    data: { body_html: '<p>pack body</p>', gallery: [], website_url: null },
  }),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modpackGetVersions: mockGetVersions,
    modpackFetchToTemp: mockFetchToTemp,
    modpackProject: mockProject,
  },
  events: {},
}));

import ModpackDetailModal from '$lib/modpacks/ModpackDetailModal.svelte';

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

describe('ModpackDetailModal', () => {
  beforeEach(() => {
    mockGetVersions.mockClear();
    mockFetchToTemp.mockClear();
    mockProject.mockClear();
  });

  it('shows the CurseForge fallback for a distribution-disabled pack', async () => {
    const hit: ModpackHit = { ...baseHit, distribution_allowed: false };
    const { findByText } = render(ModpackDetailModal, {
      props: { hit, onClose: () => {}, onInstall: () => {} },
    });
    expect(await findByText('Open on CurseForge ↗')).toBeTruthy();
  });

  it('lists versions on the Versions tab for an allowed pack', async () => {
    const hit: ModpackHit = { ...baseHit, distribution_allowed: null };
    const { findByRole } = render(ModpackDetailModal, {
      props: { hit, onClose: () => {}, onInstall: () => {} },
    });
    await waitFor(() => expect(mockGetVersions).toHaveBeenCalledWith('curseforge', 'p'));
    await fireEvent.click(await findByRole('tab', { name: 'Versions' }));
  });

  it('renders the pack description on Overview', async () => {
    const hit: ModpackHit = { ...baseHit, distribution_allowed: null };
    const { findByText } = render(ModpackDetailModal, {
      props: { hit, onClose: () => {}, onInstall: () => {} },
    });
    expect(await findByText('pack body')).toBeTruthy();
    expect(await findByText(/any ads or links in it are the author's/i)).toBeTruthy();
    expect(await findByText(/View on CurseForge/i)).toBeTruthy();
    await waitFor(() => expect(mockProject).toHaveBeenCalledWith('curseforge', 'p'));
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
    const { findByText, findByRole } = render(ModpackDetailModal, {
      props: { hit, onClose: () => {}, onInstall: () => {} },
    });
    await fireEvent.click(await findByRole('tab', { name: 'Versions' }));
    const installBtn = await findByText('Install');
    await fireEvent.click(installBtn);
    expect(await findByText('Open on CurseForge ↗')).toBeTruthy();
  });
});
