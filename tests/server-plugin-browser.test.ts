import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import ServerPluginBrowser from '$lib/servers/plugins/ServerPluginBrowser.svelte';

const {
  mockSearch,
  mockVersions,
  mockProject,
  mockInstallPlugin,
  mockListEnriched,
  mockEnablePlugin,
  mockDisablePlugin,
  mockDeletePlugin,
  mockOpenUrl,
  mockPushSuccess,
  mockPushWarning,
} = vi.hoisted(() => ({
  mockSearch: vi.fn(),
  mockVersions: vi.fn(),
  mockProject: vi.fn(),
  mockInstallPlugin: vi.fn(),
  mockListEnriched: vi.fn(),
  mockEnablePlugin: vi.fn(),
  mockDisablePlugin: vi.fn(),
  mockDeletePlugin: vi.fn(),
  mockOpenUrl: vi.fn().mockResolvedValue(undefined),
  mockPushSuccess: vi.fn(),
  mockPushWarning: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsSearch: mockSearch,
    modsPluginVersions: mockVersions,
    modsProject: mockProject,
    serverInstallPlugin: mockInstallPlugin,
    serverListPluginsEnriched: mockListEnriched,
    serverEnablePlugin: mockEnablePlugin,
    serverDisablePlugin: mockDisablePlugin,
    serverDeletePlugin: mockDeletePlugin,
  },
}));

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: mockOpenUrl }));

vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: mockPushSuccess,
  pushWarning: mockPushWarning,
}));

function hit(name: string, projectId: string) {
  return {
    source: 'modrinth' as const,
    project_id: projectId,
    slug: projectId,
    name,
    summary: 's',
    icon_url: null,
    downloads: 10,
    author: 'a',
    updated_at: null,
  };
}

function version(versionId: string, opts?: { distributionAllowed?: boolean; url?: string }) {
  return {
    source: 'modrinth' as const,
    project_id: 'we',
    version_id: versionId,
    name: versionId,
    version_number: versionId,
    mc_versions: ['1.20.1'],
    loaders: ['paper'],
    primary_file: {
      filename: `${versionId}.jar`,
      url: opts?.url ?? `https://cdn.example/${versionId}.jar`,
      distribution_allowed: opts?.distributionAllowed ?? true,
    },
    deps: [],
    published_at: null,
  };
}

// The sort control is the custom <Select> listbox: open the trigger by
// testid, then commit an option by its accessible name. The Select commits on
// `mousedown` (not click), so the option must be driven with mouseDown.
async function pickSelectOption(triggerTestid: string, name: RegExp | string) {
  await fireEvent.click(screen.getByTestId(triggerTestid));
  await fireEvent.mouseDown(screen.getByRole('option', { name }));
}

function renderBrowser(onInstalled = vi.fn()) {
  render(ServerPluginBrowser, {
    serverId: 'srv-1',
    mcVersion: '1.20.1',
    core: 'paper',
    onInstalled,
  });
  return onInstalled;
}

describe('ServerPluginBrowser', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    vi.clearAllMocks();
    mockSearch.mockResolvedValue({
      status: 'ok',
      data: { hits: [hit('WorldEdit', 'we')], total: 1 },
    });
    mockVersions.mockResolvedValue({ status: 'ok', data: [version('v9'), version('v8')] });
    // Full-project envelope for the detail card's Overview tab (opens first).
    mockProject.mockResolvedValue({
      status: 'ok',
      data: { summary: hit('WorldEdit', 'we'), body_html: '', gallery: [], website_url: null },
    });
    mockInstallPlugin.mockResolvedValue({
      status: 'ok',
      data: { installed: ['worldedit.jar'], unresolved: [] },
    });
    // Nothing installed by default: cards render the install button as before.
    mockListEnriched.mockResolvedValue({ status: 'ok', data: [] });
    mockEnablePlugin.mockResolvedValue({ status: 'ok', data: null });
    mockDisablePlugin.mockResolvedValue({ status: 'ok', data: null });
    mockDeletePlugin.mockResolvedValue({ status: 'ok', data: null });
  });

  it('searches the plugin kind with the server core (no loader facet) and renders results', async () => {
    renderBrowser();
    await waitFor(() => expect(mockSearch).toHaveBeenCalled());
    expect(mockSearch.mock.calls[0][0]).toMatchObject({
      source: 'modrinth',
      kind: 'plugin',
      mc_version: '1.20.1',
      loader: null,
      plugin_core: 'paper',
    });
    expect(await screen.findByText('WorldEdit')).toBeTruthy();
  });

  it('installs the newest version into the server and toasts the dependency count', async () => {
    mockInstallPlugin.mockResolvedValue({
      status: 'ok',
      data: { installed: ['worldedit.jar', 'dep.jar'], unresolved: [] },
    });
    const onInstalled = renderBrowser();
    const [installBtn] = await screen.findAllByLabelText('Install');
    await fireEvent.click(installBtn);
    await waitFor(() =>
      expect(mockVersions).toHaveBeenCalledWith('modrinth', 'we', '1.20.1', 'paper'),
    );
    await waitFor(() =>
      expect(mockInstallPlugin).toHaveBeenCalledWith('srv-1', 'modrinth', 'we', 'v9'),
    );
    expect(mockPushSuccess).toHaveBeenCalledWith(expect.stringContaining('1 dependency'));
    expect(onInstalled).toHaveBeenCalled();
  });

  it('opens the in-launcher detail card (not the website) when a card body is clicked', async () => {
    renderBrowser();
    const cardBody = await screen.findByRole('button', { name: /WorldEdit/ });
    await fireEvent.click(cardBody);
    // The in-launcher detail modal mounts on its Overview tab; versions live
    // behind the Versions tab.
    expect(await screen.findByTestId('server-content-detail')).toBeTruthy();
    await fireEvent.click(screen.getByRole('tab', { name: 'Versions' }));
    expect(await screen.findByText('v9')).toBeTruthy();
    // Clicking the body must NOT open the external site.
    expect(mockOpenUrl).not.toHaveBeenCalled();
  });

  it('installs the chosen version from the detail card into the server', async () => {
    renderBrowser();
    const cardBody = await screen.findByRole('button', { name: /WorldEdit/ });
    await fireEvent.click(cardBody);
    const modal = await screen.findByTestId('server-content-detail');
    // Two versions listed (v9, v8) INSIDE the modal (the grid card behind it
    // still has its own quick-install button — scope to the modal). Install
    // the older one from the picker, behind the Versions tab.
    const modalUtils = within(modal);
    await fireEvent.click(modalUtils.getByRole('tab', { name: 'Versions' }));
    const installButtons = await modalUtils.findAllByRole('button', { name: 'Install' });
    expect(installButtons).toHaveLength(2);
    await fireEvent.click(installButtons[1]);
    await waitFor(() =>
      expect(mockInstallPlugin).toHaveBeenCalledWith('srv-1', 'modrinth', 'we', 'v8'),
    );
    expect(mockPushSuccess).toHaveBeenCalled();
  });

  it('warns when the picked plugin has no compatible version', async () => {
    mockVersions.mockResolvedValue({ status: 'ok', data: [] });
    renderBrowser();
    const [installBtn] = await screen.findAllByLabelText('Install');
    await fireEvent.click(installBtn);
    await waitFor(() => expect(mockPushWarning).toHaveBeenCalled());
    expect(mockInstallPlugin).not.toHaveBeenCalled();
  });

  it('opens the external page instead of installing when distribution is disallowed', async () => {
    mockVersions.mockResolvedValue({
      status: 'ok',
      data: [version('v9', { distributionAllowed: false, url: 'https://ext.example/dl' })],
    });
    renderBrowser();
    const [installBtn] = await screen.findAllByLabelText('Install');
    await fireEvent.click(installBtn);
    await waitFor(() => expect(mockOpenUrl).toHaveBeenCalledWith('https://ext.example/dl'));
    expect(mockPushWarning).toHaveBeenCalledWith(expect.stringContaining('outside Hangar'));
    expect(mockInstallPlugin).not.toHaveBeenCalled();
  });

  it('stays on the requested page after paging (no debounced snap-back to page 1)', async () => {
    // Pins the untrack() contract in the reload effect: page changes fetch
    // once via onPage; the effect must NOT have picked up `page` as a
    // dependency during its synchronous first run (which would re-fire it and
    // debounce a page=0 reload ~250ms after every page click).
    mockSearch.mockResolvedValue({
      status: 'ok',
      data: { hits: [hit('WorldEdit', 'we')], total: 1000 },
    });
    renderBrowser();
    await waitFor(() => expect(mockSearch).toHaveBeenCalledTimes(1));
    const pageSize = mockSearch.mock.calls[0][0].page_size as number;
    expect(mockSearch.mock.calls[0][0].offset).toBe(0);

    const next = await screen.findByTestId('pg-next');
    await fireEvent.click(next);
    await waitFor(() => expect(mockSearch).toHaveBeenCalledTimes(2));
    expect(mockSearch.mock.calls[1][0].offset).toBe(pageSize);

    // Outwait the 250ms debounce window; a tracked-page regression would fire
    // a third search with offset 0 here.
    await new Promise((resolve) => setTimeout(resolve, 350));
    expect(mockSearch).toHaveBeenCalledTimes(2);
  });

  it('changing sort re-runs the search with the new sort and resets to page 0', async () => {
    renderBrowser();
    await waitFor(() => expect(mockSearch).toHaveBeenCalled());
    mockSearch.mockClear();
    await pickSelectOption('server-plugin-sort', 'Updated');
    await waitFor(() => expect(mockSearch).toHaveBeenCalled());
    expect(mockSearch.mock.calls[0][0]).toMatchObject({
      sort: 'updated',
      offset: 0,
      plugin_core: 'paper',
      loader: null,
    });
  });

  it('renders the installed state (no re-install button) for an already-installed plugin', async () => {
    // The enriched registry reports WorldEdit as installed → the card shows the
    // Remove/toggle controls instead of an Install button, so it can't be
    // installed again (the repeat-install toast-spam bug this fixes).
    mockListEnriched.mockResolvedValue({
      status: 'ok',
      data: [
        {
          filename: 'worldedit.jar',
          on_disk_filename: 'worldedit.jar',
          disabled: false,
          sha1: 'sha-we',
          source: 'modrinth',
          project_id: 'we',
          version_id: 'v9',
          name: 'WorldEdit',
          version_number: 'v9',
        },
      ],
    });
    renderBrowser();
    expect(await screen.findByLabelText('Remove')).toBeTruthy();
    await waitFor(() => expect(screen.queryByLabelText('Install')).toBeNull());
  });
});
