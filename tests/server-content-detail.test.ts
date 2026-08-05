import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import ServerContentDetail from '$lib/servers/browser/ServerContentDetail.svelte';

// The modal is content-kind-agnostic: it takes the project + injected callbacks
// (loadProject / loadVersions / installVersion / externalOf / openExternal /
// onInstalled) and never imports the kind-specific commands itself. So the tests
// inject vi.fn()s and assert the wiring — no IPC mock needed for the commands.
// We only stub format-error so the error path yields a predictable string.
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

// The full-project envelope for the Overview tab. Defaults to an empty body +
// empty gallery so the Overview falls back to the project summary and shows no
// gallery box; individual tests override `body_html` / `gallery`.
function okProject(data: Record<string, unknown> = {}) {
  return {
    status: 'ok' as const,
    data: { summary: project(), body_html: '', gallery: [], website_url: null, ...data },
  };
}

function mountDetail(overrides: Record<string, unknown> = {}) {
  const props = {
    project: project(),
    onClose: vi.fn(),
    loadProject: vi.fn().mockResolvedValue(okProject()),
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

// The modal defaults to the Overview tab (parity with the client ModDetailModal),
// so version-list assertions must first switch to the Versions tab.
async function showVersions() {
  await fireEvent.click(screen.getByRole('tab', { name: 'Versions' }));
}

describe('ServerContentDetail', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => vi.clearAllMocks());

  describe('Versions tab', () => {
    it('loads versions on open and renders a row per version', async () => {
      const props = mountDetail();
      await waitFor(() => expect(props.loadVersions).toHaveBeenCalled());
      await showVersions();
      expect(await screen.findByText('7.2.0')).toBeTruthy();
      expect(screen.getByText('7.1.0')).toBeTruthy();
      // Header shows the project name + a project-page link (both tabs).
      expect(screen.getByText('WorldEdit')).toBeTruthy();
      expect(screen.getByText('Open project page')).toBeTruthy();
    });

    it('installs the chosen version (not just the newest) and reports back', async () => {
      const props = mountDetail();
      await showVersions();
      await screen.findByText('7.1.0');
      // Click the SECOND row's Install — the older version.
      const installButtons = screen.getAllByRole('button', { name: 'Install' });
      expect(installButtons).toHaveLength(2);
      await fireEvent.click(installButtons[1]);
      // installVersion receives the whole chosen version object, not just its
      // id — the datapack browser's install command needs the full
      // ModVersion, and handing back the id alone forced a by-id cache with
      // no request-sequencing guard (see ServerContentDetail's prop doc).
      await waitFor(() =>
        expect(props.installVersion).toHaveBeenCalledWith(version('v8', '7.1.0')),
      );
      await waitFor(() =>
        expect(props.onInstalled).toHaveBeenCalledWith(
          { installed: ['worldedit.jar'], unresolved: [] },
          '7.1.0',
        ),
      );
      // On success the modal closes.
      expect(props.onClose).toHaveBeenCalled();
    });

    it('shows Open page (not Install) for an externally hosted version and opens it', async () => {
      const external = version('vx', '5.0.0');
      const props = mountDetail({
        loadVersions: vi.fn().mockResolvedValue(okVersions([external])),
        externalOf: vi.fn().mockReturnValue('https://ext.example/dl'),
      });
      await showVersions();
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
      await showVersions();
      expect(await screen.findByText('Loading versions…')).toBeTruthy();
      resolve(okVersions([version('v9', '7.2.0')]));
      expect(await screen.findByText('7.2.0')).toBeTruthy();
    });

    it('renders the empty state when there are no versions', async () => {
      mountDetail({ loadVersions: vi.fn().mockResolvedValue(okVersions([])) });
      await showVersions();
      expect(await screen.findByText('No versions for this server.')).toBeTruthy();
    });

    it('surfaces an inline error when loading versions fails', async () => {
      mountDetail({
        loadVersions: vi.fn().mockResolvedValue({
          status: 'error',
          error: { kind: 'mods_platform_unreachable', url: 'x' },
        }),
      });
      await showVersions();
      expect(await screen.findByTestId('server-content-detail-error')).toBeTruthy();
    });

    it('surfaces an inline error when the install fails, and does not close', async () => {
      const props = mountDetail({
        installVersion: vi.fn().mockResolvedValue({
          status: 'error',
          error: { kind: 'mods_platform_unreachable', url: 'x' },
        }),
      });
      await showVersions();
      await screen.findByText('7.2.0');
      const [firstInstall] = screen.getAllByRole('button', { name: 'Install' });
      await fireEvent.click(firstInstall);
      expect(await screen.findByTestId('server-content-detail-error')).toBeTruthy();
      expect(props.onClose).not.toHaveBeenCalled();
      expect(props.onInstalled).not.toHaveBeenCalled();
    });
  });

  describe('Overview tab', () => {
    it('fetches the project on open and renders the description body', async () => {
      const props = mountDetail({
        loadProject: vi
          .fn()
          .mockResolvedValue(okProject({ body_html: '<h1>WorldEdit</h1><p>Edit fast.</p>' })),
      });
      await waitFor(() => expect(props.loadProject).toHaveBeenCalled());
      // Body HTML is rendered (via RenderedBody) and the source disclaimer shows.
      expect(await screen.findByText('Edit fast.')).toBeTruthy();
      expect(screen.getByText(/Content from Modrinth/)).toBeTruthy();
    });

    it('falls back to the project summary when the body is empty', async () => {
      mountDetail(); // default okProject has an empty body_html
      expect(await screen.findByText('In-game map editor.')).toBeTruthy();
    });

    it('renders the screenshot gallery when the project has images', async () => {
      mountDetail({
        loadProject: vi
          .fn()
          .mockResolvedValue(
            okProject({ gallery: [{ url: 'https://img.example/a.png', title: 'A' }] }),
          ),
      });
      const img = await screen.findByRole('img');
      expect(img.getAttribute('src')).toBe('https://img.example/a.png');
    });

    it('renders no gallery box when the project has no images', async () => {
      mountDetail(); // default gallery is empty
      await screen.findByText('In-game map editor.');
      // The header uses an icon fallback (no <img>), so the only possible <img>
      // would come from a gallery — there must be none.
      expect(screen.queryByRole('img')).toBeNull();
    });

    it('shows the description loading state before the project arrives', async () => {
      let resolve: (v: unknown) => void = () => {};
      const pending = new Promise((r) => (resolve = r));
      mountDetail({ loadProject: vi.fn().mockReturnValue(pending) });
      expect(await screen.findByText('Loading description…')).toBeTruthy();
      resolve(okProject({ body_html: '<p>Done.</p>' }));
      expect(await screen.findByText('Done.')).toBeTruthy();
    });

    it('surfaces an error when the project fails to load', async () => {
      mountDetail({
        loadProject: vi.fn().mockResolvedValue({
          status: 'error',
          error: { kind: 'mods_platform_unreachable', url: 'x' },
        }),
      });
      expect(await screen.findByTestId('server-content-detail-overview-error')).toBeTruthy();
    });
  });

  it('opens the project page from the header link', async () => {
    const props = mountDetail();
    await fireEvent.click(screen.getByText('Open project page'));
    expect(props.openExternal).toHaveBeenCalledWith('https://modrinth.com/mod/worldedit');
  });
});
