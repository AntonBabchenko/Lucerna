// Busy-state coverage for InstalledAssetsView — verifies the Check-updates
// button shows a spinner (Spinner role="status") while the update-check runs
// and clears it once the check resolves. The check keys off the `checking`
// flag, separate from the per-asset `busy` flag.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstalledAsset } from '$lib/ipc/bindings';
import { assetsChanged } from '$lib/settings/state.svelte';

const { assetsList, assetUninstall, assetsCheckUpdates, assetInstall, pushWarning, pushSuccess } =
  vi.hoisted(() => ({
    assetsList: vi.fn(),
    assetUninstall: vi.fn(),
    assetsCheckUpdates: vi.fn(),
    assetInstall: vi.fn(),
    pushWarning: vi.fn(),
    pushSuccess: vi.fn(),
  }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    assetsList,
    assetUninstall,
    assetsCheckUpdates,
    assetInstall,
  },
}));

vi.mock('$lib/toasts/toasts.svelte', () => ({ pushWarning, pushSuccess }));

import InstalledAssetsView from '$lib/mods/InstalledAssetsView.svelte';

function makeAsset(over: Partial<InstalledAsset> = {}): InstalledAsset {
  return {
    kind: 'resource_pack',
    filename: 'pack.zip',
    sha1: 'aaa',
    source: 'modrinth',
    project_id: 'proj-1',
    version_id: 'ver-1',
    name: 'Faithful',
    version_number: '1.0',
    installed_at: '2026-06-01T00:00:00Z',
    ...over,
  };
}

function ok<T>(data: T) {
  return { status: 'ok' as const, data };
}

describe('InstalledAssetsView busy state', () => {
  beforeEach(() => {
    assetsList.mockReset();
    assetUninstall.mockReset();
    assetsCheckUpdates.mockReset();
    assetInstall.mockReset();
    pushWarning.mockReset();
    pushSuccess.mockReset();
    assetsChanged.value = 0;
  });

  afterEach(() => {
    assetsChanged.value = 0;
  });

  it('spins the Check-updates button while the check runs and clears it on resolve', async () => {
    assetsList.mockResolvedValue(ok([makeAsset()]));
    // A deferred promise we control — the check stays in flight until we resolve.
    let resolveCheck!: (v: unknown) => void;
    assetsCheckUpdates.mockReturnValue(
      new Promise((res) => {
        resolveCheck = res as (v: unknown) => void;
      }),
    );

    render(InstalledAssetsView, { props: { instanceId: 'inst-1', kind: 'resource_pack' } });

    // Wait for the list to populate so the Check-updates button is enabled.
    await screen.findByText('Faithful');
    const btn = screen.getByRole('button', { name: /check.*update/i });
    expect(btn.querySelector('[role="status"]')).toBeNull();

    await fireEvent.click(btn);

    // Mid-flight: the spinner is present.
    await waitFor(() => expect(btn.querySelector('[role="status"]')).not.toBeNull());

    // Resolve the check → spinner clears.
    resolveCheck(ok([]));
    await waitFor(() => expect(btn.querySelector('[role="status"]')).toBeNull());
  });
});
