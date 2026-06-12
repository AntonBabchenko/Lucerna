import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
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
    modpackFetchToTemp: mockFetchToTemp,
    modpackProject: mockProject,
  },
  events: {    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
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
};

describe('ModpackDetailModal install busy state', () => {
  beforeEach(() => {
    mockGetVersions.mockClear();
    mockFetchToTemp.mockClear();
    mockProject.mockClear();
  });
  afterEach(() => {
    vi.clearAllMocks();
  });

  it('spins the recommended install button while fetching the pack', async () => {
    // A never-resolving fetch keeps the button mid-flight so we can observe
    // the spinner, then we resolve it to confirm the spinner clears.
    let resolveFetch!: (v: unknown) => void;
    mockFetchToTemp.mockReturnValueOnce(
      new Promise((res) => {
        resolveFetch = res;
      }),
    );

    const { findByRole } = render(ModpackDetailModal, {
      props: { hit: baseHit, onClose: () => {}, onInstall: () => {} },
    });

    // Overview footer renders the recommended-version install button.
    const btn = await findByRole('button', { name: /^Install/ });
    expect(btn.querySelector('[role="status"]')).toBeNull();

    await fireEvent.click(btn);

    await waitFor(() => expect(btn.querySelector('[role="status"]')).not.toBeNull());
    expect(btn.getAttribute('aria-busy')).toBe('true');
    expect((btn as HTMLButtonElement).disabled).toBe(true);

    resolveFetch({ status: 'ok', data: '/tmp/pack' });

    await waitFor(() => expect(btn.querySelector('[role="status"]')).toBeNull());
    expect(btn.getAttribute('aria-busy')).toBe('false');
  });

  it('spins the per-version install button on the Versions tab', async () => {
    let resolveFetch!: (v: unknown) => void;
    mockFetchToTemp.mockReturnValueOnce(
      new Promise((res) => {
        resolveFetch = res;
      }),
    );

    const { findByRole } = render(ModpackDetailModal, {
      props: { hit: baseHit, onClose: () => {}, onInstall: () => {} },
    });

    await fireEvent.click(await findByRole('tab', { name: 'Versions' }));
    const btn = await findByRole('button', { name: 'Install' });
    expect(btn.querySelector('[role="status"]')).toBeNull();

    await fireEvent.click(btn);

    await waitFor(() => expect(btn.querySelector('[role="status"]')).not.toBeNull());

    resolveFetch({ status: 'ok', data: '/tmp/pack' });
    await waitFor(() => expect(btn.querySelector('[role="status"]')).toBeNull());
  });
});
