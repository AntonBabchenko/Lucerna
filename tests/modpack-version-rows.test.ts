import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { ModpackHit } from '$lib/ipc/bindings';

const { mockGetVersions, mockFetchToTemp, mockProject } = vi.hoisted(() => ({
  mockGetVersions: vi.fn().mockResolvedValue({
    status: 'ok',
    data: [
      {
        id: 'v1',
        name: 'Pack 14.0.0-beta.4',
        version_number: '14.0.0-beta.4',
        game_versions: ['1.20.1'],
        loaders: ['neoforge'],
        date_published: '2026-01-01T00:00:00Z',
      },
      {
        id: 'v2',
        name: 'Pack 13.0.0',
        version_number: '13.0.0',
        game_versions: ['1.19.2'],
        loaders: [],
        date_published: '2025-01-01T00:00:00Z',
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

const hit: ModpackHit = {
  project_id: 'p',
  slug: 'atm9',
  title: 'ATM9',
  description: 'desc',
  icon_url: null,
  downloads: 1,
  latest_mc_version: null,
  supported_loaders: [],
  source: 'curseforge',
  distribution_allowed: null,
  author: null,
};

async function openVersionsTab() {
  render(ModpackDetailModal, {
    props: { hit, mcFilter: null, onClose: () => {}, onInstall: () => {} },
  });
  await fireEvent.click(await screen.findByText('Versions'));
}

describe('ModpackDetailModal version rows', () => {
  it('spells out Minecraft and the loader display name', async () => {
    await openVersionsTab();
    await waitFor(() => expect(screen.getByText('Minecraft 1.20.1 · NeoForge')).toBeTruthy());
  });

  it('leaves no dangling separator when a version reports no loader', async () => {
    await openVersionsTab();
    await waitFor(() => expect(screen.getByText('Minecraft 1.19.2')).toBeTruthy());
  });

  it('names the version in the row install action, per DESIGN.md §5', async () => {
    await openVersionsTab();
    await waitFor(() =>
      expect(screen.getByLabelText('Install 14.0.0-beta.4 as a new instance')).toBeTruthy(),
    );
  });
});
