// Behaviour coverage for InstalledAssetsView — the simpler resource-pack /
// shader sibling of the mods Installed view. Verifies: row render, Remove
// wiring, Check-updates → Update wiring, and the pick-instance empty state.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstalledAsset, ModVersion } from '$lib/ipc/bindings';
import { assetsChanged } from '$lib/settings/state.svelte';

// vi.mock is hoisted — the spies live in vi.hoisted so the factory and the
// tests share the same references.
const {
  assetsList,
  assetUninstall,
  assetsCheckUpdates,
  assetInstall,
  assetUpdateOne,
  modsProjects,
  pushWarning,
  pushSuccess,
} = vi.hoisted(() => ({
  assetsList: vi.fn(),
  assetUninstall: vi.fn(),
  assetsCheckUpdates: vi.fn(),
  assetInstall: vi.fn(),
  assetUpdateOne: vi.fn(),
  modsProjects: vi.fn(),
  pushWarning: vi.fn(),
  pushSuccess: vi.fn(),
}));

const spies = {
  assetsList,
  assetUninstall,
  assetsCheckUpdates,
  assetInstall,
  assetUpdateOne,
  modsProjects,
};

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    assetsList,
    assetUninstall,
    assetsCheckUpdates,
    assetInstall,
    assetUpdateOne,
    modsProjects,
  },
}));

vi.mock('$lib/toasts/toasts.svelte', () => ({ pushWarning, pushSuccess }));

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

function makeSummary() {
  return {
    source: 'modrinth' as const,
    project_id: 'proj-1',
    slug: 'complementary',
    name: 'Complementary Shaders',
    summary: 'A shader pack',
    icon_url: 'https://x/i.png',
    downloads: 0,
    author: 'EminGT',
    updated_at: null,
  };
}

function ok<T>(data: T) {
  return { status: 'ok' as const, data };
}

// `assets_list`'s answer: the registry's files and the unpacked folders (`null`: unreadable).
function listing(files: InstalledAsset[], folders: string[] | null = []) {
  return ok({ files, folders });
}

describe('InstalledAssetsView', () => {
  beforeEach(() => {
    for (const s of Object.values(spies)) s.mockReset();
    pushWarning.mockReset();
    pushSuccess.mockReset();
    // Enrichment resolves each platform asset's project summary (icon). Default
    // to a resolved summary so the icon loads; individual tests can override.
    modsProjects.mockResolvedValue(ok([makeSummary()]));
    // The assetsChanged rune is module-global and shared across tests — reset
    // it so a bump from one test can't leak into the next.
    assetsChanged.value = 0;
  });

  afterEach(() => {
    assetsChanged.value = 0;
  });

  it('renders an asset row with its name and a Remove button', async () => {
    spies.assetsList.mockResolvedValue(listing([makeAsset()]));

    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });

    expect(await screen.findByText('Complementary Shaders')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Remove' })).toBeTruthy();
    expect(spies.assetsList).toHaveBeenCalledWith('inst-1', 'shader');
  });

  it('loads the project icon for a platform asset', async () => {
    spies.assetsList.mockResolvedValue(listing([makeAsset()]));

    const { container } = render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });

    await screen.findByText('Complementary Shaders');
    await waitFor(() =>
      expect(container.querySelector('img')?.getAttribute('src')).toBe('https://x/i.png'),
    );
  });

  it('calls assetUninstall with (instanceId, kind, filename) on Remove', async () => {
    spies.assetsList.mockResolvedValue(listing([makeAsset()]));
    spies.assetUninstall.mockResolvedValue(ok(null));

    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });

    await screen.findByText('Complementary Shaders');
    await fireEvent.click(screen.getByRole('button', { name: 'Remove' }));

    expect(spies.assetUninstall).toHaveBeenCalledWith('inst-1', 'shader', 'complementary.zip');
  });

  it('shows an Update button after Check updates and updates via assetUpdateOne', async () => {
    const latest = makeVersion();
    spies.assetsList.mockResolvedValue(listing([makeAsset()]));
    spies.assetsCheckUpdates.mockResolvedValue(
      ok([
        {
          filename: 'complementary.zip',
          name: 'Complementary Shaders',
          state: { kind: 'update_available', latest },
        },
      ]),
    );
    spies.assetUpdateOne.mockResolvedValue(ok({ old_file_kept: null }));

    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });

    await screen.findByText('Complementary Shaders');
    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));

    const updateBtn = await screen.findByRole('button', { name: 'Update' });
    expect(updateBtn).toBeTruthy();

    await fireEvent.click(updateBtn);

    // The update must replace the installed file (old filename passed for
    // cleanup), not merely install the new version alongside it.
    await waitFor(() =>
      expect(spies.assetUpdateOne).toHaveBeenCalledWith(
        'inst-1',
        'shader',
        'complementary.zip',
        latest,
      ),
    );
    expect(spies.assetInstall).not.toHaveBeenCalled();
  });

  it('shows Update all with the update count and updates every updatable row', async () => {
    const latestA = makeVersion();
    const latestB = makeVersion({
      project_id: 'proj-2',
      version_id: 'ver-9',
      name: 'Faithful',
      primary_file: {
        filename: 'faithful-1.21.zip',
        url: 'https://example.test/f.zip',
        sha1: 'ccc',
        size: 1,
        distribution_allowed: true,
      },
    });
    spies.assetsList.mockResolvedValue(
      listing([
        makeAsset(),
        makeAsset({ filename: 'faithful.zip', project_id: 'proj-2', name: 'Faithful' }),
      ]),
    );
    spies.assetsCheckUpdates.mockResolvedValue(
      ok([
        {
          filename: 'complementary.zip',
          name: 'Complementary Shaders',
          state: { kind: 'update_available', latest: latestA },
        },
        {
          filename: 'faithful.zip',
          name: 'Faithful',
          state: { kind: 'update_available', latest: latestB },
        },
      ]),
    );
    spies.assetUpdateOne.mockResolvedValue(ok({ old_file_kept: null }));

    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });

    await screen.findByText('Complementary Shaders');
    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));

    const updateAllBtn = await screen.findByRole('button', { name: 'Update all (2)' });
    await fireEvent.click(updateAllBtn);

    await waitFor(() => expect(spies.assetUpdateOne).toHaveBeenCalledTimes(2));
    expect(spies.assetUpdateOne).toHaveBeenCalledWith(
      'inst-1',
      'shader',
      'complementary.zip',
      latestA,
    );
    expect(spies.assetUpdateOne).toHaveBeenCalledWith('inst-1', 'shader', 'faithful.zip', latestB);
    await waitFor(() => expect(pushSuccess).toHaveBeenCalled());
  });

  it('reports a partial Update all failure with a warning toast and stays alive', async () => {
    const latestA = makeVersion();
    const latestB = makeVersion({ project_id: 'proj-2', version_id: 'ver-9' });
    spies.assetsList.mockResolvedValue(
      listing([
        makeAsset(),
        makeAsset({ filename: 'faithful.zip', project_id: 'proj-2', name: 'Faithful' }),
      ]),
    );
    spies.assetsCheckUpdates.mockResolvedValue(
      ok([
        {
          filename: 'complementary.zip',
          name: 'Complementary Shaders',
          state: { kind: 'update_available', latest: latestA },
        },
        {
          filename: 'faithful.zip',
          name: 'Faithful',
          state: { kind: 'update_available', latest: latestB },
        },
      ]),
    );
    spies.assetUpdateOne
      .mockResolvedValueOnce(ok({ old_file_kept: null }))
      .mockResolvedValueOnce({ status: 'error' as const, error: 'network error' });

    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });

    await screen.findByText('Complementary Shaders');
    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Update all (2)' }));

    await waitFor(() => expect(pushWarning).toHaveBeenCalled());
    expect(screen.queryByText('Complementary Shaders')).toBeTruthy();
  });

  it('does not render Update all when nothing is updatable', async () => {
    spies.assetsList.mockResolvedValue(listing([makeAsset()]));
    spies.assetsCheckUpdates.mockResolvedValue(
      ok([
        {
          filename: 'complementary.zip',
          name: 'Complementary Shaders',
          state: { kind: 'up_to_date' },
        },
      ]),
    );

    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });

    await screen.findByText('Complementary Shaders');
    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
    await waitFor(() => expect(pushSuccess).toHaveBeenCalled()); // up-to-date toast
    expect(screen.queryByRole('button', { name: /Update all/ })).toBeNull();
  });

  it('renders the pick-instance empty state and does not list when instanceId is null', async () => {
    render(InstalledAssetsView, { instanceId: null, kind: 'shader' });

    expect(await screen.findByText('Pick an instance first.')).toBeTruthy();
    expect(spies.assetsList).not.toHaveBeenCalled();
  });

  it('calls pushWarning and does not remove the row when assetUninstall returns an error', async () => {
    spies.assetsList.mockResolvedValue(listing([makeAsset()]));
    spies.assetUninstall.mockResolvedValue({ status: 'error' as const, error: 'disk full' });

    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });

    await screen.findByText('Complementary Shaders');
    await fireEvent.click(screen.getByRole('button', { name: 'Remove' }));

    await waitFor(() => expect(pushWarning).toHaveBeenCalled());
    // Row must still be present after error
    expect(screen.queryByText('Complementary Shaders')).toBeTruthy();
  });

  it('calls pushWarning and does not crash when assetsCheckUpdates returns an error', async () => {
    spies.assetsList.mockResolvedValue(listing([makeAsset()]));
    spies.assetsCheckUpdates.mockResolvedValue({
      status: 'error' as const,
      error: 'network error',
    });

    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });

    await screen.findByText('Complementary Shaders');
    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));

    await waitFor(() => expect(pushWarning).toHaveBeenCalled());
    // Component must still be alive — the row is still present
    expect(screen.queryByText('Complementary Shaders')).toBeTruthy();
  });

  it('resets checking to false when assetsCheckUpdates throws (IPC error)', async () => {
    spies.assetsList.mockResolvedValue(listing([makeAsset()]));
    spies.assetsCheckUpdates.mockRejectedValue(new Error('ipc failure'));

    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });

    await screen.findByText('Complementary Shaders');
    const checkBtn = screen.getByRole('button', { name: 'Check for updates' });

    // Before click — button enabled (not disabled).
    expect(checkBtn.hasAttribute('disabled')).toBe(false);

    await fireEvent.click(checkBtn);

    // After the rejected promise settles, checking must have been reset so the
    // button is no longer permanently stuck in the disabled state.
    await waitFor(() => expect(checkBtn.hasAttribute('disabled')).toBe(false));
  });

  it('shows the check_failed warning indicator when an asset check fails', async () => {
    const reason = 'Project was delisted from Modrinth';
    spies.assetsList.mockResolvedValue(listing([makeAsset()]));
    spies.assetsCheckUpdates.mockResolvedValue(
      ok([
        {
          filename: 'complementary.zip',
          name: 'Complementary Shaders',
          state: { kind: 'check_failed', reason },
        },
      ]),
    );

    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });

    await screen.findByText('Complementary Shaders');
    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));

    // The ⚠ indicator is rendered for the check_failed state. The reason is
    // delivered via use:tooltip (not a native title attribute) — tooltip delivery
    // is covered by the tooltip unit tests and e2e; here we confirm the indicator
    // element is present and has the right accessible label.
    const indicator = await screen.findByRole('img', { name: 'Update check failed' });
    expect(indicator).toBeTruthy();
  });

  it('refetches assetsList when the shared assetsChanged signal is bumped', async () => {
    // Start with an empty list — simulates the bug: a pack installed from Browse
    // exists on disk but the Installed view was rendered before it landed.
    spies.assetsList.mockResolvedValue(listing([] as InstalledAsset[]));

    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });

    expect(await screen.findByText('Nothing installed in this instance yet.')).toBeTruthy();
    expect(spies.assetsList).toHaveBeenCalledTimes(1);

    // A pack is now installed elsewhere (Browse / a producer) → the next list
    // call should surface it. Bumping the signal must drive a refetch.
    spies.assetsList.mockResolvedValue(listing([makeAsset()]));
    assetsChanged.value++;

    expect(await screen.findByText('Complementary Shaders')).toBeTruthy();
    expect(spies.assetsList).toHaveBeenCalledTimes(2);
  });

  // The dropzone rule (DESIGN.md §14): an empty list holds the host's full drop area and says so
  // — the host hides its strip meanwhile. A list that could not be read is not empty.
  const hostDropzone = () =>
    createRawSnippet(() => ({ render: () => '<div data-testid="host-dropzone"></div>' }));

  it('reports an empty list to its host and renders the host’s drop area in it', async () => {
    spies.assetsList.mockResolvedValue(listing([] as InstalledAsset[]));
    const onEmptyChange = vi.fn();
    render(InstalledAssetsView, {
      instanceId: 'inst-1',
      kind: 'shader',
      emptyDropzone: hostDropzone(),
      onEmptyChange,
    });
    expect(await screen.findByTestId('host-dropzone')).toBeTruthy();
    expect(screen.getByTestId('list-empty').textContent).toContain(
      'Nothing installed in this instance yet.',
    );
    expect(onEmptyChange).toHaveBeenLastCalledWith(true);
  });

  it('a list that could not be read claims nothing: no «nothing installed», no drop area', async () => {
    spies.assetsList.mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: 'shaderpacks', details: 'denied' },
    });
    const onEmptyChange = vi.fn();
    render(InstalledAssetsView, {
      instanceId: 'inst-1',
      kind: 'shader',
      emptyDropzone: hostDropzone(),
      onEmptyChange,
    });
    expect(await screen.findByText(/denied/)).toBeTruthy();
    expect(screen.queryByText('Nothing installed in this instance yet.')).toBeNull();
    expect(screen.queryByTestId('host-dropzone')).toBeNull();
    expect(onEmptyChange).not.toHaveBeenCalledWith(true);
  });
  // ── Packs placed by hand (the folder is the truth) ──────────────────────────

  it('lists a folder pack after the files, tagged as a folder in words', async () => {
    spies.assetsList.mockResolvedValue(listing([makeAsset()], ['Dev Pack']));
    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });
    await screen.findByText('Dev Pack');
    const rows = screen.getAllByTestId('asset-row');
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain('Complementary Shaders');
    expect(rows[1].textContent).toContain('Dev Pack');
    expect(rows[1].textContent).toContain('folder');
  });

  it('removing a folder pack says it went to the recycle bin', async () => {
    spies.assetsList.mockResolvedValue(listing([], ['Dev Pack']));
    spies.assetUninstall.mockResolvedValue(ok(null));
    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'resource_pack' });
    await screen.findByText('Dev Pack');
    await fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    await waitFor(() =>
      expect(spies.assetUninstall).toHaveBeenCalledWith('inst-1', 'resource_pack', 'Dev Pack'),
    );
    expect(pushSuccess).toHaveBeenCalledWith('Moved Dev Pack to the recycle bin');
    await waitFor(() => expect(screen.queryByText('Dev Pack')).toBeNull());
  });

  it('a folder that could not be read is said above the list, which still shows', async () => {
    spies.assetsList.mockResolvedValue(listing([makeAsset()], null));
    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });
    expect(await screen.findByTestId('asset-folder-unreadable')).toBeTruthy();
    expect(screen.getAllByTestId('asset-row')).toHaveLength(1);
  });

  it('an unreadable folder with no rows does not claim the list is empty', async () => {
    spies.assetsList.mockResolvedValue(listing([], null));
    const onEmptyChange = vi.fn();
    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader', onEmptyChange });
    expect(await screen.findByTestId('asset-folder-unreadable')).toBeTruthy();
    expect(screen.queryByTestId('list-empty')).toBeNull();
    expect(onEmptyChange).not.toHaveBeenCalledWith(true);
  });

  it('re-reads when the window regains focus while it is the list on screen', async () => {
    spies.assetsList.mockResolvedValue(listing([makeAsset()]));
    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader', visible: true });
    await screen.findByText('Complementary Shaders');
    const before = spies.assetsList.mock.calls.length;
    window.dispatchEvent(new Event('focus'));
    await waitFor(() => expect(spies.assetsList.mock.calls.length).toBe(before + 1));
  });

  it('does not re-read in the Servers panel, where the client list is mounted but hidden', async () => {
    const { serversUi } = await import('$lib/servers/servers-ui.svelte');
    spies.assetsList.mockResolvedValue(listing([makeAsset()]));
    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader', visible: true });
    await screen.findByText('Complementary Shaders');
    const before = spies.assetsList.mock.calls.length;
    serversUi.mode = 'servers';
    try {
      await new Promise((r) => setTimeout(r, 0));
      window.dispatchEvent(new Event('focus'));
      await new Promise((r) => setTimeout(r, 0));
      expect(spies.assetsList.mock.calls.length).toBe(before);
    } finally {
      serversUi.mode = 'client';
    }
  });

  // A return during a removal is not dropped: the list reads it once the removal is done.
  it('a return during a removal is read once the removal ends', async () => {
    let finishRemove!: (v: unknown) => void;
    spies.assetsList.mockResolvedValue(listing([makeAsset()], ['Dev Pack']));
    spies.assetUninstall.mockImplementation(() => new Promise((r) => (finishRemove = r)));
    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader', visible: true });
    await screen.findByText('Dev Pack');
    const removeButtons = screen.getAllByRole('button', { name: 'Remove' });
    await fireEvent.click(removeButtons[removeButtons.length - 1]);
    const before = spies.assetsList.mock.calls.length;
    window.dispatchEvent(new Event('focus'));
    await new Promise((r) => setTimeout(r, 0));
    expect(spies.assetsList.mock.calls.length).toBe(before);
    finishRemove(ok(null));
    await waitFor(() => expect(spies.assetsList.mock.calls.length).toBe(before + 1));
  });

  // The loading panel is up only while nothing is listed: an empty answer shows whether `loading`
  // was cleared, because the empty list (and the host's drop area) needs it cleared.
  it('a focus re-read that overtakes the first read does not leave the list loading', async () => {
    let releaseFirst!: (v: unknown) => void;
    spies.assetsList
      .mockImplementationOnce(() => new Promise((r) => (releaseFirst = r)))
      .mockResolvedValue(listing([]));
    const onEmptyChange = vi.fn();
    render(InstalledAssetsView, {
      instanceId: 'inst-1',
      kind: 'shader',
      visible: true,
      onEmptyChange,
    });
    window.dispatchEvent(new Event('focus'));
    expect(await screen.findByTestId('list-empty')).toBeTruthy();
    expect(onEmptyChange).toHaveBeenLastCalledWith(true);
    releaseFirst(listing([]));
  });

  it('a read that succeeds after a failed one clears its error', async () => {
    spies.assetsList
      .mockResolvedValueOnce({
        status: 'error',
        error: { kind: 'io', path: 'p', details: 'denied' },
      })
      .mockResolvedValue(listing([makeAsset()]));
    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader', visible: true });
    await screen.findByText(/denied/);
    window.dispatchEvent(new Event('focus'));
    await screen.findByText('Complementary Shaders');
    expect(screen.queryByText(/denied/)).toBeNull();
  });

  it('an update that leaves the old file on disk says so instead of a plain «Updated»', async () => {
    const latest = makeVersion();
    spies.assetsList.mockResolvedValue(listing([makeAsset()]));
    spies.assetsCheckUpdates.mockResolvedValue(
      ok([
        {
          filename: 'complementary.zip',
          name: 'Complementary Shaders',
          state: { kind: 'update_available', latest },
        },
      ]),
    );
    spies.assetUpdateOne.mockResolvedValue(ok({ old_file_kept: 'in use by another process' }));
    render(InstalledAssetsView, { instanceId: 'inst-1', kind: 'shader' });
    await screen.findByText('Complementary Shaders');
    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Update' }));
    await waitFor(() =>
      expect(pushWarning).toHaveBeenCalledWith(
        'Updated Complementary Shaders, but the old file complementary.zip is still in the folder',
        ['in use by another process'],
      ),
    );
    expect(pushSuccess).not.toHaveBeenCalledWith('Updated Complementary Shaders');
  });
});
