import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstanceWithStatus, ModpackUpdateDiff, ModpackVersionEntry } from '$lib/ipc/bindings';

// vi.mock is hoisted above top-level consts, so the mock fns must come from
// vi.hoisted or collection fails with "Cannot access '…' before initialization".
const { getVersions, fetchToTemp, computeUpdate, applyUpdate, changelog } = vi.hoisted(() => ({
  getVersions: vi.fn(),
  fetchToTemp: vi.fn(),
  computeUpdate: vi.fn(),
  applyUpdate: vi.fn(),
  changelog: vi.fn(),
}));

// The apply path builds Tauri progress Channels, which need the IPC runtime.
// Same stub the other modpack tests use.
vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: ((m: unknown) => void) | null = null;
  },
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modpackGetVersions: getVersions,
    modpackFetchToTemp: fetchToTemp,
    modpackComputeUpdate: computeUpdate,
    modpackApplyUpdate: applyUpdate,
    modsChangelog: changelog,
  },
}));

import ModpackVersionSwitchDialog from '$lib/modpacks/ModpackVersionSwitchDialog.svelte';

function ver(id: string, date: string): ModpackVersionEntry {
  return {
    id,
    name: `Release ${id}`,
    version_number: id,
    game_versions: ['1.20.1'],
    loaders: ['fabric'],
    date_published: date,
  };
}

const VERSIONS = [ver('v3', '2026-03-01T00:00:00Z'), ver('v1', '2026-01-01T00:00:00Z')];

const DIFF: ModpackUpdateDiff = {
  added: [],
  removed: [],
  updated: [],
  version_bump: null,
  new_version_number: 'v1',
};

// Installed = v3 (the newest), so picking v1 is a downgrade.
const inst = {
  id: 'inst-1',
  mrpack_name: 'RLCraft',
  mrpack_project_id: 'proj',
  mrpack_source: 'modrinth',
  mrpack_version_id: 'v3',
  mrpack_version: 'v3',
} as unknown as InstanceWithStatus;

const props = () => ({
  inst,
  userAdded: 0,
  manual: 0,
  hasBundledFiles: false,
  onClose: vi.fn(),
  onSwitched: vi.fn(),
});

const NETWORK_ERROR = {
  status: 'error' as const,
  error: { kind: 'mods_network' as const, url: 'https://x', details: 'HTTP 503' },
};

beforeEach(() => {
  vi.clearAllMocks();
  getVersions.mockResolvedValue({ status: 'ok', data: VERSIONS });
  fetchToTemp.mockResolvedValue({ status: 'ok', data: '/tmp/pack.mrpack' });
  computeUpdate.mockResolvedValue({ status: 'ok', data: DIFF });
  applyUpdate.mockResolvedValue({ status: 'ok', data: inst });
  changelog.mockResolvedValue({ status: 'ok', data: { sections: [], truncated: null } });
});

async function openAndPick(id: string) {
  render(ModpackVersionSwitchDialog, props());
  await waitFor(() => expect(screen.getByTestId(`version-row-${id}`)).toBeTruthy());
  await fireEvent.click(screen.getByTestId(`version-row-${id}`));
}

describe('ModpackVersionSwitchDialog', () => {
  it('loads and lists the pack versions', async () => {
    render(ModpackVersionSwitchDialog, props());
    await waitFor(() => expect(screen.getByTestId('version-row-v1')).toBeTruthy());
    expect(getVersions).toHaveBeenCalledWith('modrinth', 'proj');
  });

  it('moves to the review step and shows the diff when a version is picked', async () => {
    await openAndPick('v1');
    await waitFor(() => expect(screen.getByTestId('update-diff-list')).toBeTruthy());
    expect(fetchToTemp).toHaveBeenCalledWith('modrinth', 'proj', 'v1');
  });

  it('warns about the downgrade when the picked version is older', async () => {
    await openAndPick('v1');
    await waitFor(() => expect(screen.getByTestId('switch-risk-downgrade')).toBeTruthy());
  });

  it('does not warn about a downgrade when picking the newest version', async () => {
    await openAndPick('v3');
    await waitFor(() => expect(screen.getByTestId('update-diff-list')).toBeTruthy());
    expect(screen.queryByTestId('switch-risk-downgrade')).toBeNull();
  });

  it('applies the CHOSEN version, not the latest', async () => {
    await openAndPick('v1');
    await waitFor(() => expect(screen.getByTestId('switch-confirm')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('switch-confirm'));
    await waitFor(() => expect(applyUpdate).toHaveBeenCalled());
    // (instanceId, mrpackPath, newVersionId, …)
    expect(applyUpdate.mock.calls[0][2]).toBe('v1');
  });

  it('notifies the caller after a successful switch', async () => {
    const p = props();
    render(ModpackVersionSwitchDialog, p);
    await waitFor(() => expect(screen.getByTestId('version-row-v1')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('version-row-v1'));
    await waitFor(() => expect(screen.getByTestId('switch-confirm')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('switch-confirm'));
    await waitFor(() => expect(p.onSwitched).toHaveBeenCalled());
  });

  it('shows the cause and a retry when preparing fails', async () => {
    fetchToTemp.mockResolvedValue(NETWORK_ERROR);
    await openAndPick('v1');
    await waitFor(() => expect(screen.getByTestId('switch-error')).toBeTruthy());
    expect(screen.getByTestId('switch-retry')).toBeTruthy();
  });

  it('retrying re-runs the prepare for the same version', async () => {
    fetchToTemp.mockResolvedValueOnce(NETWORK_ERROR);
    await openAndPick('v1');
    await waitFor(() => expect(screen.getByTestId('switch-retry')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('switch-retry'));
    await waitFor(() => expect(fetchToTemp).toHaveBeenCalledTimes(2));
    expect(fetchToTemp.mock.calls[1][2]).toBe('v1');
  });

  it('surfaces a version-list load failure', async () => {
    getVersions.mockResolvedValue(NETWORK_ERROR);
    render(ModpackVersionSwitchDialog, props());
    await waitFor(() => expect(screen.getByTestId('switch-error')).toBeTruthy());
  });

  it('offers the changelog on the review step', async () => {
    await openAndPick('v1');
    await waitFor(() => expect(screen.getByTestId('switch-changelog-btn')).toBeTruthy());
  });

  it('takes the version list away while a pick is being prepared', async () => {
    // Fetching the archive takes seconds. If the list stayed clickable, a slow
    // first prepare could land its temp path against a second version's id —
    // applying one version's files while recording the other's.
    let release!: (v: unknown) => void;
    fetchToTemp.mockReturnValue(
      new Promise((resolve) => {
        release = resolve;
      }),
    );
    await openAndPick('v1');
    await waitFor(() => expect(screen.queryByTestId('version-row-v3')).toBeNull());
    expect(screen.queryByTestId('version-row-v1')).toBeNull();
    release({ status: 'ok', data: '/tmp/pack.mrpack' });
    await waitFor(() => expect(screen.getByTestId('update-diff-list')).toBeTruthy());
  });

  it('goes back to the version list from review', async () => {
    await openAndPick('v1');
    await waitFor(() => expect(screen.getByTestId('switch-back')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('switch-back'));
    await waitFor(() => expect(screen.getByTestId('version-row-v1')).toBeTruthy());
    expect(screen.queryByTestId('update-diff-list')).toBeNull();
  });
});
