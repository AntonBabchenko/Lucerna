import { render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { ModpackHit } from '$lib/ipc/bindings';

const { mockGetVersions, mockFetchToTemp, mockProject } = vi.hoisted(() => ({
  mockGetVersions: vi.fn(),
  mockFetchToTemp: vi.fn(),
  mockProject: vi.fn().mockResolvedValue({
    status: 'ok',
    data: { body_html: null, gallery: [], website_url: null },
  }),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modpackGetVersions: mockGetVersions,
    modpackProject: mockProject,
    modpackFetchToTemp: mockFetchToTemp,
  },
  events: { gpuPrefApplied: { listen: () => Promise.resolve(() => {}) } },
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
  source: 'modrinth',
  distribution_allowed: null,
  author: null,
};

function versions(over: Record<string, unknown> = {}) {
  return {
    status: 'ok',
    data: [
      {
        id: 'v1',
        name: 'Pack 14.0.0-beta.4',
        version_number: '14.0.0-beta.4',
        game_versions: ['1.20.1'],
        loaders: ['fabric'],
        date_published: '2026-01-01T00:00:00Z',
        ...over,
      },
    ],
  };
}

function open(mcFilter: string | null) {
  render(ModpackDetailModal, {
    props: { hit: baseHit, mcFilter, onClose: () => {}, onInstall: () => {} },
  });
}

describe('ModpackDetailModal install CTA', () => {
  it('names the version, why it was picked, the MC version and the loader', async () => {
    mockGetVersions.mockResolvedValue(versions());
    open(null);
    await waitFor(() => expect(screen.getByText('Install 14.0.0-beta.4')).toBeTruthy());
    expect(screen.getByTestId('modpack-install-summary').textContent?.trim()).toBe(
      'Newest version · Minecraft 1.20.1 · Fabric',
    );
  });

  it('says the pick came from the active MC filter', async () => {
    mockGetVersions.mockResolvedValue(versions());
    open('1.20.1');
    await waitFor(() =>
      expect(screen.getByTestId('modpack-install-summary').textContent?.trim()).toBe(
        'Newest matching your filter · Minecraft 1.20.1 · Fabric',
      ),
    );
  });

  // A CurseForge pack version whose file names no loader must not render a
  // dangling separator — the segment disappears entirely.
  it('drops the loader segment when the source reports none', async () => {
    mockGetVersions.mockResolvedValue(versions({ loaders: [] }));
    open(null);
    await waitFor(() =>
      expect(screen.getByTestId('modpack-install-summary').textContent?.trim()).toBe(
        'Newest version · Minecraft 1.20.1',
      ),
    );
  });
});
