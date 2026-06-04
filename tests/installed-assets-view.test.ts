// Behaviour coverage for InstalledAssetsView — the simpler resource-pack /
// shader sibling of the mods Installed view. Verifies: row render, Remove
// wiring, Check-updates → Update wiring, and the pick-instance empty state.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstalledAsset, ModVersion } from '$lib/ipc/bindings';

// vi.mock is hoisted — the spies live in vi.hoisted so the factory and the
// tests share the same references.
const spies = vi.hoisted(() => ({
  assetsList: vi.fn(),
  assetUninstall: vi.fn(),
  assetsCheckUpdates: vi.fn(),
  assetInstall: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    assetsList: spies.assetsList,
    assetUninstall: spies.assetUninstall,
    assetsCheckUpdates: spies.assetsCheckUpdates,
    assetInstall: spies.assetInstall,
  },
}));

import InstalledAssetsView from '$lib/mods/InstalledAssetsView.svelte';

// ── Fixtures ─────────────────────────────────────────────────────────────────

function makeAsset(over: Partial<InstalledAsset> = {}): InstalledAsset {
  return {
    kind: 'shader',
    filename: 'complementary.zip',
    sha1: 'aaa',
    source: 'modrinth',
    project_id: 'proj-1',
    version_id: 'ver-1',
    name: 'Complementary Shaders',
    version_number: 'r5.0',
    installed_at: '2026-06-01T00:00:00Z',
    ...over,
  };
}

function makeVersion(over: Partial<ModVersion> = {}): ModVersion {
  return {
    source: 'modrinth',
    project_id: 'proj-1',
    version_id: 'ver-2',
    name: 'Complementary Shaders',
    version_number: 'r5.1',
    mc_versions: ['1.20.4'],
    loaders: [],
    primary_file: {
      filename: 'complementary-r5.1.zip',
      url: 'https://example.test/x.zip',
      sha1: 'bbb',
      size: 1,
      distribution_allowed: true,
    },
    deps: [],
    published_at: null,
    ...over,
  };
}

function ok<T>(data: T) {
  return { status: 'ok' as const, data };
}

describe('InstalledAssetsView', () => {
  beforeEach(() => {
    for (const s of Object.values(spies)) s.mockReset();
  });

  it('renders an asset row with its name and a Remove button', async () => {
    spies.assetsList.mockResolvedValue(ok([makeAsset()]));

    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });

    expect(await screen.findByText('Complementary Shaders')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Remove' })).toBeTruthy();
    expect(spies.assetsList).toHaveBeenCalledWith('inst-1', 'shader');
  });

  it('calls assetUninstall with (instanceId, kind, filename) on Remove', async () => {
    spies.assetsList.mockResolvedValue(ok([makeAsset()]));
    spies.assetUninstall.mockResolvedValue(ok(null));

    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });

    await screen.findByText('Complementary Shaders');
    await fireEvent.click(screen.getByRole('button', { name: 'Remove' }));

    expect(spies.assetUninstall).toHaveBeenCalledWith('inst-1', 'shader', 'complementary.zip');
  });

  it('shows an Update button after Check updates and installs the latest version', async () => {
    const latest = makeVersion();
    spies.assetsList.mockResolvedValue(ok([makeAsset()]));
    spies.assetsCheckUpdates.mockResolvedValue(
      ok([
        {
          filename: 'complementary.zip',
          name: 'Complementary Shaders',
          state: { kind: 'update_available', latest },
        },
      ]),
    );
    spies.assetInstall.mockResolvedValue(ok(null));

    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });

    await screen.findByText('Complementary Shaders');
    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));

    const updateBtn = await screen.findByRole('button', { name: 'Update' });
    expect(updateBtn).toBeTruthy();

    await fireEvent.click(updateBtn);

    await waitFor(() =>
      expect(spies.assetInstall).toHaveBeenCalledWith('inst-1', latest, 'shader'),
    );
  });

  it('renders the pick-instance empty state and does not list when instanceId is null', async () => {
    render(InstalledAssetsView, { instanceId: null, kind: 'shader' });

    expect(await screen.findByText('Pick an instance first.')).toBeTruthy();
    expect(spies.assetsList).not.toHaveBeenCalled();
  });
});
