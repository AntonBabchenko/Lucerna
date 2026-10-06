import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { InstanceWithStatus, ModpackUpdateDiff, ModpackVersionEntry } from '$lib/ipc/bindings';

// vi.mock is hoisted above top-level consts, so the mock fns must come from
// vi.hoisted or collection fails with "Cannot access '…' before initialization".
const { getVersions, fetchToTemp, computeUpdate, applyUpdate, changelog, worldNames } = vi.hoisted(
  () => ({
    getVersions: vi.fn(),
    fetchToTemp: vi.fn(),
    computeUpdate: vi.fn(),
    applyUpdate: vi.fn(),
    changelog: vi.fn(),
    worldNames: vi.fn(),
  }),
);

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
    listWorldNames: worldNames,
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
  applyUpdate.mockResolvedValue({
    status: 'ok',
    data: { instance: inst, inert_loader_jars: [], details: [] },
  });
  changelog.mockResolvedValue({ status: 'ok', data: { sections: [], truncated: null } });
  worldNames.mockResolvedValue({ status: 'ok', data: [] });
});

async function openAndPick(id: string) {
  render(ModpackVersionSwitchDialog, props());
  await waitFor(() => expect(screen.getByTestId(`version-row-${id}`)).toBeTruthy());
  await fireEvent.click(screen.getByTestId(`version-row-${id}`));
}

/** Rendered, a version picked, and the review step up. */
async function openOnReview(id: string) {
  const p = props();
  render(ModpackVersionSwitchDialog, p);
  await waitFor(() => expect(screen.getByTestId(`version-row-${id}`)).toBeTruthy());
  await fireEvent.click(screen.getByTestId(`version-row-${id}`));
  await waitFor(() => expect(screen.getByTestId('switch-confirm')).toBeTruthy());
  return p;
}

/** A press AND a release on the shared Modal's scrim (the dialog's parent). */
async function clickBackdrop() {
  const scrim = screen.getAllByRole('dialog')[0].parentElement as HTMLElement;
  await fireEvent.mouseDown(scrim);
  await fireEvent.mouseUp(scrim);
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

  it('a busy refusal of the switch says why, offers a retry, and reports no switch', async () => {
    // The apply takes the instance's maintenance claim, so while the game
    // runs or starts, or a mod migration, world migration or clone holds the
    // instance, the backend refuses with InstanceBusy before touching a file.
    // Nothing switched: the caller must not be told it did, and a retry must
    // re-prepare the SAME version so its diff is recomputed against whatever
    // the other operation left behind.
    locale.set('en');
    applyUpdate.mockResolvedValueOnce({ status: 'error', error: { kind: 'instance_busy' } });
    const p = props();
    render(ModpackVersionSwitchDialog, p);
    await waitFor(() => expect(screen.getByTestId('version-row-v1')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('version-row-v1'));
    await waitFor(() => expect(screen.getByTestId('switch-confirm')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('switch-confirm'));

    const error = await screen.findByTestId('switch-error');
    expect(error.textContent).toContain(
      'An operation is already in progress, or the game is running.',
    );
    expect(p.onSwitched).not.toHaveBeenCalled();

    await fireEvent.click(screen.getByTestId('switch-retry'));
    await waitFor(() => expect(fetchToTemp).toHaveBeenCalledTimes(2));
    expect(fetchToTemp.mock.calls[1][2]).toBe('v1');
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

  it('offers a retry when the version list itself failed to load', async () => {
    // The failure happens before any pick, so a retry gated on "a version is
    // selected" would leave closing and reopening the dialog as the only
    // recovery — and nothing in the UI hints at that.
    getVersions.mockResolvedValue(NETWORK_ERROR);
    render(ModpackVersionSwitchDialog, props());
    await waitFor(() => expect(screen.getByTestId('switch-retry')).toBeTruthy());
  });

  it('retrying a failed version-list load re-fetches the list', async () => {
    getVersions.mockResolvedValueOnce(NETWORK_ERROR);
    render(ModpackVersionSwitchDialog, props());
    await waitFor(() => expect(screen.getByTestId('switch-retry')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('switch-retry'));
    // Retries the LOAD, not a prepare — nothing has been picked yet.
    await waitFor(() => expect(getVersions).toHaveBeenCalledTimes(2));
    expect(fetchToTemp).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.getByTestId('version-row-v1')).toBeTruthy());
  });

  // Each step takes the focused control with it — the picked row, Retry, Back, the confirm — and
  // the next one opened with the focus on <body>: nothing announced, Enter doing nothing. The
  // dialog's panel takes the focus instead (Modal's stepKey; 2026-10-06 regression, O4).
  describe('keyboard focus across steps', () => {
    const dialog = () => screen.getByRole('dialog');

    it('stays in the dialog from the picked version to the review', async () => {
      render(ModpackVersionSwitchDialog, props());
      await waitFor(() => expect(screen.getByTestId('version-row-v1')).toBeTruthy());
      const row = screen.getByTestId('version-row-v1');
      row.focus();
      await fireEvent.click(row);
      await waitFor(() => expect(screen.getByTestId('switch-confirm')).toBeTruthy());
      expect(document.activeElement).toBe(dialog());
    });

    it('stays in the dialog when Back returns to the list', async () => {
      await openOnReview('v1');
      const back = screen.getByTestId('switch-back');
      back.focus();
      await fireEvent.click(back);
      await waitFor(() => expect(screen.getByTestId('version-row-v1')).toBeTruthy());
      expect(document.activeElement).toBe(dialog());
    });

    it('stays in the dialog while the confirmed switch applies', async () => {
      let finish: (v: unknown) => void = () => {};
      applyUpdate.mockReturnValue(new Promise((resolve) => (finish = resolve)));
      try {
        await openOnReview('v1');
        const confirm = screen.getByTestId('switch-confirm');
        confirm.focus();
        await fireEvent.click(confirm);
        await waitFor(() => expect(screen.queryByTestId('switch-confirm')).toBeNull());
        expect(document.activeElement).toBe(dialog());
      } finally {
        // A held apply must be given back, red or green, or it outlives the test.
        finish({ status: 'ok', data: { instance: inst, inert_loader_jars: [], details: [] } });
      }
    });

    it('stays in the dialog when Retry reloads the version list', async () => {
      getVersions.mockResolvedValueOnce(NETWORK_ERROR);
      render(ModpackVersionSwitchDialog, props());
      await waitFor(() => expect(screen.getByTestId('switch-retry')).toBeTruthy());
      const retry = screen.getByTestId('switch-retry');
      retry.focus();
      await fireEvent.click(retry);
      await waitFor(() => expect(screen.getByTestId('version-row-v1')).toBeTruthy());
      expect(document.activeElement).toBe(dialog());
    });
  });

  it('does not show an empty-list state when the load failed', async () => {
    // `versions` is still [] after a failure, so the list's "no versions match"
    // copy would read as "this pack has none" rather than "the request failed".
    getVersions.mockResolvedValue(NETWORK_ERROR);
    render(ModpackVersionSwitchDialog, props());
    await waitFor(() => expect(screen.getByTestId('switch-error')).toBeTruthy());
    expect(screen.queryByTestId('version-list-empty')).toBeNull();
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

  // Escape and the backdrop dismiss a dialog. On the review step nothing runs —
  // the flow only holds the fetched archive — so they leave it exactly like
  // Back + close. The review step used to swallow both, leaving Back as the
  // only way out.
  it('closes on Escape from the review step, dropping the picked version', async () => {
    const p = await openOnReview('v1');
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(p.onClose).toHaveBeenCalledTimes(1);
    // Left the way Back leaves, through the flow. The drawer unmounts the
    // dialog on close, so no user sees the list come back; it is how this test
    // sees that the exit dropped the fetched version in the flow, where any
    // cleanup of it belongs, rather than only hiding it.
    await waitFor(() => expect(screen.getByTestId('version-row-v1')).toBeTruthy());
    expect(screen.queryByTestId('update-diff-list')).toBeNull();
  });

  it('closes on a backdrop click from the review step', async () => {
    const p = await openOnReview('v1');
    await clickBackdrop();
    expect(p.onClose).toHaveBeenCalledTimes(1);
  });

  it('Escape over the changelog closes the changelog first, then the dialog', async () => {
    const p = await openOnReview('v1');
    await fireEvent.click(screen.getByTestId('switch-changelog-btn'));
    await waitFor(() => expect(screen.getAllByRole('dialog')).toHaveLength(2));
    await fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => expect(screen.getAllByRole('dialog')).toHaveLength(1));
    expect(p.onClose).not.toHaveBeenCalled();
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(p.onClose).toHaveBeenCalledTimes(1);
  });

  it('stays open while the picked version is being prepared', async () => {
    let land!: (v: unknown) => void;
    fetchToTemp.mockReturnValue(
      new Promise((resolve) => {
        land = resolve;
      }),
    );
    const p = props();
    render(ModpackVersionSwitchDialog, p);
    await waitFor(() => expect(screen.getByTestId('version-row-v1')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('version-row-v1'));
    await waitFor(() => expect(screen.queryByTestId('version-row-v1')).toBeNull());
    await fireEvent.keyDown(window, { key: 'Escape' });
    await clickBackdrop();
    const closes = p.onClose.mock.calls.length;
    land({ status: 'ok', data: '/tmp/pack.mrpack' });
    expect(closes).toBe(0);
    await waitFor(() => expect(screen.getByTestId('update-diff-list')).toBeTruthy());
  });

  it('stays open while the switch is being applied', async () => {
    let land!: (v: unknown) => void;
    applyUpdate.mockReturnValue(
      new Promise((resolve) => {
        land = resolve;
      }),
    );
    const p = await openOnReview('v1');
    await fireEvent.click(screen.getByTestId('switch-confirm'));
    const progressShown = screen.queryByTestId('imported-detail-updating') !== null;
    await fireEvent.keyDown(window, { key: 'Escape' });
    await clickBackdrop();
    const closes = p.onClose.mock.calls.length;
    // Released before anything is asserted, so a failure here never leaves the
    // serial task lane blocked for the tests below.
    land({ status: 'ok', data: { instance: inst, inert_loader_jars: [], details: [] } });
    await waitFor(() => expect(p.onSwitched).toHaveBeenCalled());
    expect(progressShown).toBe(true);
    expect(closes).toBe(0);
  });

  // A bridge failure used to escape the prepare and leave it running for
  // good: the dialog, which stays open while running, could not be closed.
  it('says why and can be closed when preparing fails at the bridge', async () => {
    fetchToTemp.mockRejectedValue(new Error('bridge down'));
    const p = props();
    render(ModpackVersionSwitchDialog, p);
    await waitFor(() => expect(screen.getByTestId('version-row-v1')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('version-row-v1'));
    const error = await screen.findByTestId('switch-error');
    expect(error.textContent).toContain('bridge down');
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(p.onClose).toHaveBeenCalledTimes(1);
  });

  it('closes on Escape from the version list', async () => {
    const p = props();
    render(ModpackVersionSwitchDialog, p);
    await waitFor(() => expect(screen.getByTestId('version-row-v1')).toBeTruthy());
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(p.onClose).toHaveBeenCalledTimes(1);
  });

  it("closes from the version list's Cancel", async () => {
    locale.set('en');
    const p = props();
    render(ModpackVersionSwitchDialog, p);
    await waitFor(() => expect(screen.getByTestId('version-row-v1')).toBeTruthy());
    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(p.onClose).toHaveBeenCalledTimes(1);
  });

  it('goes back to the version list from review', async () => {
    await openAndPick('v1');
    await waitFor(() => expect(screen.getByTestId('switch-back')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('switch-back'));
    await waitFor(() => expect(screen.getByTestId('version-row-v1')).toBeTruthy());
    expect(screen.queryByTestId('update-diff-list')).toBeNull();
  });

  it('offers a world backup at review and passes the choice on', async () => {
    worldNames.mockResolvedValue({
      status: 'ok',
      data: [
        { folder_name: 'A', modified_unix_ms: 0 },
        { folder_name: 'B', modified_unix_ms: 0 },
      ],
    });
    await openAndPick('v1');
    const box = (await screen.findByTestId('switch-backup-worlds')) as HTMLInputElement;
    // No Minecraft change and nothing removed: offered, not preselected.
    expect(box.checked).toBe(false);
    expect(box.closest('label')?.textContent).toContain('(2)');
    await fireEvent.click(box);
    await fireEvent.click(screen.getByTestId('switch-confirm'));
    await waitFor(() => expect(applyUpdate).toHaveBeenCalled());
    expect(applyUpdate.mock.calls[0][3]).toBe(true);
  });

  it('hides the backup choice on a profile without worlds', async () => {
    await openAndPick('v1');
    await waitFor(() => expect(screen.getByTestId('switch-confirm')).toBeTruthy());
    expect(screen.queryByTestId('switch-backup-worlds')).toBeNull();
    await fireEvent.click(screen.getByTestId('switch-confirm'));
    await waitFor(() => expect(applyUpdate).toHaveBeenCalled());
    expect(applyUpdate.mock.calls[0][3]).toBe(false);
  });
});
