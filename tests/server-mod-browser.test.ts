import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import ServerModBrowser from '$lib/servers/mods/ServerModBrowser.svelte';

const {
  mockSearch,
  mockVersions,
  mockProject,
  mockInstallMod,
  mockCfStatus,
  mockOpenUrl,
  mockPushSuccess,
  mockPushWarning,
} = vi.hoisted(() => ({
  mockSearch: vi.fn(),
  mockVersions: vi.fn(),
  mockProject: vi.fn(),
  mockInstallMod: vi.fn(),
  mockCfStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'present' }),
  mockOpenUrl: vi.fn().mockResolvedValue(undefined),
  mockPushSuccess: vi.fn(),
  mockPushWarning: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsSearch: mockSearch,
    modsVersions: mockVersions,
    modsProject: mockProject,
    serverInstallMod: mockInstallMod,
    modsGetCurseforgeKeyStatus: mockCfStatus,
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

describe('ServerModBrowser', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    vi.clearAllMocks();
    mockCfStatus.mockResolvedValue({ status: 'ok', data: 'present' });
    mockSearch.mockResolvedValue({ status: 'ok', data: { hits: [hit('JEI', 'jei')], total: 1 } });
    // Full-project envelope for the detail card's Overview tab (opens first).
    mockProject.mockResolvedValue({
      status: 'ok',
      data: { summary: hit('JEI', 'jei'), body_html: '', gallery: [], website_url: null },
    });
    mockVersions.mockResolvedValue({
      status: 'ok',
      data: [
        {
          version_id: 'v9',
          project_id: 'jei',
          source: 'modrinth',
          name: '15.0.0',
          version_number: '15.0.0',
          mc_versions: ['1.20.1'],
          loaders: ['forge'],
          primary_file: {
            filename: 'jei.jar',
            url: 'https://cdn/jei.jar',
            distribution_allowed: true,
          },
          deps: [],
          published_at: null,
        },
      ],
    });
    mockInstallMod.mockResolvedValue({
      status: 'ok',
      data: { installed: ['jei.jar'], unresolved: [] },
    });
  });

  it('searches for the server mc + loader and renders results', async () => {
    render(ServerModBrowser, {
      serverId: 'srv-1',
      mcVersion: '1.20.1',
      loader: 'forge',
      onInstalled: vi.fn(),
    });
    await waitFor(() => expect(mockSearch).toHaveBeenCalled());
    expect(mockSearch.mock.calls[0][0]).toMatchObject({
      kind: 'mod',
      mc_version: '1.20.1',
      loader: 'forge',
    });
    expect(await screen.findByText('JEI')).toBeTruthy();
  });

  it('installs the newest version into the server with its dependency closure', async () => {
    const onInstalled = vi.fn();
    render(ServerModBrowser, {
      serverId: 'srv-1',
      mcVersion: '1.20.1',
      loader: 'forge',
      onInstalled,
    });
    const [installBtn] = await screen.findAllByLabelText('Install');
    await fireEvent.click(installBtn);
    await waitFor(() =>
      expect(mockInstallMod).toHaveBeenCalledWith('srv-1', 'modrinth', 'jei', 'v9'),
    );
    expect(mockPushSuccess).toHaveBeenCalled();
    expect(onInstalled).toHaveBeenCalled();
  });

  it('opens the project page instead of installing a restricted (non-distributable) mod', async () => {
    mockVersions.mockResolvedValue({
      status: 'ok',
      data: [
        {
          version_id: 'v9',
          project_id: 'jei',
          source: 'modrinth',
          name: '15.0.0',
          version_number: '15.0.0',
          mc_versions: ['1.20.1'],
          loaders: ['forge'],
          // Author disabled third-party downloads: no fetchable file.
          primary_file: { filename: 'jei.jar', url: '', distribution_allowed: false },
          deps: [],
          published_at: null,
        },
      ],
    });
    render(ServerModBrowser, {
      serverId: 'srv-1',
      mcVersion: '1.20.1',
      loader: 'forge',
      onInstalled: vi.fn(),
    });
    const [installBtn] = await screen.findAllByLabelText('Install');
    await fireEvent.click(installBtn);
    await waitFor(() => expect(mockOpenUrl).toHaveBeenCalled());
    expect(mockPushWarning).toHaveBeenCalled();
    expect(mockInstallMod).not.toHaveBeenCalled();
  });

  it('opens the in-launcher detail card (not the website) when a card body is clicked', async () => {
    mockVersions.mockResolvedValue({
      status: 'ok',
      data: [
        {
          version_id: 'v9',
          project_id: 'jei',
          source: 'modrinth',
          name: '15.0.0',
          version_number: '15.0.0',
          mc_versions: ['1.20.1'],
          loaders: ['forge'],
          primary_file: {
            filename: 'jei.jar',
            url: 'https://cdn/jei.jar',
            distribution_allowed: true,
          },
          deps: [],
          published_at: null,
        },
      ],
    });
    render(ServerModBrowser, {
      serverId: 'srv-1',
      mcVersion: '1.20.1',
      loader: 'forge',
      onInstalled: vi.fn(),
    });
    // The card body button carries the project name.
    const cardBody = await screen.findByRole('button', { name: /JEI/ });
    await fireEvent.click(cardBody);
    // The in-launcher detail modal mounts on its Overview tab…
    expect(await screen.findByTestId('server-content-detail')).toBeTruthy();
    // …and the Versions tab lists the fetched version.
    await fireEvent.click(screen.getByRole('tab', { name: 'Versions' }));
    expect(await screen.findByText('15.0.0')).toBeTruthy();
    expect(mockOpenUrl).not.toHaveBeenCalled();
  });

  it('warns when the picked mod has no compatible version', async () => {
    mockVersions.mockResolvedValue({ status: 'ok', data: [] });
    render(ServerModBrowser, {
      serverId: 'srv-1',
      mcVersion: '1.20.1',
      loader: 'forge',
      onInstalled: vi.fn(),
    });
    const [installBtn] = await screen.findAllByLabelText('Install');
    await fireEvent.click(installBtn);
    await waitFor(() => expect(mockPushWarning).toHaveBeenCalled());
    expect(mockInstallMod).not.toHaveBeenCalled();
  });
});
