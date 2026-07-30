import { render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { ModpackHit } from '$lib/ipc/bindings';

const { mockGetVersions, mockFetchToTemp, mockProject } = vi.hoisted(() => ({
  mockGetVersions: vi.fn().mockResolvedValue({
    status: 'ok',
    data: [
      {
        id: 'v1',
        name: 'Pack 1.0',
        version_number: '1.0',
        game_versions: ['1.20.1'],
        loaders: ['fabric'],
        date_published: '2026-01-01T00:00:00Z',
      },
    ],
  }),
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

describe('ModpackDetailModal scope note', () => {
  it('warns above the install CTA that the pack lands in a new instance', async () => {
    render(ModpackDetailModal, {
      props: {
        hit: baseHit,
        mcFilter: null,
        onClose: () => {},
        onInstall: () => {},
      },
    });

    await waitFor(() => {
      expect(screen.getByTestId('modpack-detail-scope-note').textContent).toContain('new instance');
    });
  });
});
