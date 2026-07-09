import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import ServerContentDetail from '$lib/servers/browser/ServerContentDetail.svelte';

// The modal is content-kind-agnostic: it takes the project + injected callbacks
// (loadVersions / installVersion / externalOf / openExternal / onInstalled) and
// never imports the kind-specific commands itself. So the tests inject vi.fn()s
// and assert the wiring — no IPC mock needed for the commands. We only stub
// format-error so the error path yields a predictable string.
vi.mock('$lib/ipc/format-error', () => ({
  formatError: () => 'boom',
}));

function project(overrides: Record<string, unknown> = {}) {
  return {
    source: 'modrinth' as const,
    project_id: 'we',
    slug: 'worldedit',
    name: 'WorldEdit',
    summary: 'In-game map editor.',
    icon_url: null,
    downloads: 1234,
    author: 'sk89q',
    updated_at: null,
    ...overrides,
  };
}

function version(versionId: string, versionNumber: string, mc: string[] = ['1.20.1']) {
  return {
    source: 'modrinth' as const,
    project_id: 'we',
    version_id: versionId,
    name: versionNumber,
    version_number: versionNumber,
    mc_versions: mc,
    loaders: ['paper'],
    primary_file: {
      filename: `${versionId}.jar`,
      url: `https://cdn.example/${versionId}.jar`,
      distribution_allowed: true,
    },
    deps: [],
    published_at: null,
  };
}

function okVersions(data: unknown[]) {
  return { status: 'ok' as const, data };
}

function mountDetail(overrides: Record<string, unknown> = {}) {
  const props = {
    project: project(),
    onClose: vi.fn(),
    loadVersions: vi
      .fn()
      .mockResolvedValue(okVersions([version('v9', '7.2.0'), version('v8', '7.1.0')])),
    installVersion: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { installed: ['worldedit.jar'], unresolved: [] } }),
    externalOf: vi.fn().mockReturnValue(null),
    openExternal: vi.fn(),
    projectUrl: 'https://modrinth.com/mod/worldedit',
    onInstalled: vi.fn(),
    ...overrides,
  };
  render(ServerContentDetail, props);
  return props;
}

describe('ServerContentDetail', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => vi.clearAllMocks());

  it('loads versions on open and renders a row per version', async () => {
    const props = mountDetail();
    await waitFor(() => expect(props.loadVersions).toHaveBeenCalled());
    expect(await screen.findByText('7.2.0')).toBeTruthy();
    expect(screen.getByText('7.1.0')).toBeTruthy();
    // Header shows the project name + a project-page link.
    expect(screen.getByText('WorldEdit')).toBeTruthy();
    expect(screen.getByText('Open project page')).toBeTruthy();
  });

  it('installs the chosen version (not just the newest) and reports back', async () => {
    const props = mountDetail();
    await screen.findByText('7.1.0');
    // Click the SECOND row's Install — the older version.
    const installButtons = screen.getAllByRole('button', { name: 'Install' });
    expect(installButtons).toHaveLength(2);
    await fireEvent.click(installButtons[1]);
    await waitFor(() => expect(props.installVersion).toHaveBeenCalledWith('v8'));
    await waitFor(() =>
      expect(props.onInstalled).toHaveBeenCalledWith(
        { installed: ['worldedit.jar'], unresolved: [] },
        '7.1.0',
      ),
    );
    // On success the modal closes.
    expect(props.onClose).toHaveBeenCalled();
  });

  it('opens the project page from the header link', async () => {
    const props = mountDetail();
    await screen.findByText('7.2.0');
    await fireEvent.click(screen.getByText('Open project page'));
    expect(props.openExternal).toHaveBeenCalledWith('https://modrinth.com/mod/worldedit');
  });

  it('shows Open page (not Install) for an externally hosted version and opens it', async () => {
    const external = version('vx', '5.0.0');
    const props = mountDetail({
      loadVersions: vi.fn().mockResolvedValue(okVersions([external])),
      externalOf: vi.fn().mockReturnValue('https://ext.example/dl'),
    });
    await screen.findByText('5.0.0');
    expect(screen.queryByRole('button', { name: 'Install' })).toBeNull();
    const openPage = screen.getByRole('button', { name: /Open page/ });
    await fireEvent.click(openPage);
    expect(props.openExternal).toHaveBeenCalledWith('https://ext.example/dl');
    expect(props.installVersion).not.toHaveBeenCalled();
  });

  it('renders the loading state before versions arrive', async () => {
    let resolve: (v: unknown) => void = () => {};
    const pending = new Promise((r) => (resolve = r));
    mountDetail({ loadVersions: vi.fn().mockReturnValue(pending) });
    expect(await screen.findByText('Loading versions…')).toBeTruthy();
    resolve(okVersions([version('v9', '7.2.0')]));
    expect(await screen.findByText('7.2.0')).toBeTruthy();
  });

  it('renders the empty state when there are no versions', async () => {
    mountDetail({ loadVersions: vi.fn().mockResolvedValue(okVersions([])) });
    expect(await screen.findByText('No versions for this server.')).toBeTruthy();
  });

  it('surfaces an inline error when loading versions fails', async () => {
    mountDetail({
      loadVersions: vi.fn().mockResolvedValue({
        status: 'error',
        error: { kind: 'mods_platform_unreachable', url: 'x' },
      }),
    });
    expect(await screen.findByTestId('server-content-detail-error')).toBeTruthy();
  });

  it('surfaces an inline error when the install fails, and does not close', async () => {
    const props = mountDetail({
      installVersion: vi.fn().mockResolvedValue({
        status: 'error',
        error: { kind: 'mods_platform_unreachable', url: 'x' },
      }),
    });
    await screen.findByText('7.2.0');
    const [firstInstall] = screen.getAllByRole('button', { name: 'Install' });
    await fireEvent.click(firstInstall);
    expect(await screen.findByTestId('server-content-detail-error')).toBeTruthy();
    expect(props.onClose).not.toHaveBeenCalled();
    expect(props.onInstalled).not.toHaveBeenCalled();
  });
});
