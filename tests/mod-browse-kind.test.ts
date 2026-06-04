import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

// Mock the IPC layer the same way mod-browse-view.test.ts does: every
// command returns the tauri-specta `{ status, data | error }` envelope.
// These tests exercise ModBrowseView's new `kind` prop, so we additionally
// stub modsVersions + assetInstall (the resource-pack / shader install path)
// and modsInstallWithDeps (the mod path we assert is NOT taken).
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsSearch: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { hits: [], total: 0, offset: 0, page_size: 20 } }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsProject: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        summary: {
          source: 'modrinth',
          project_id: 'p',
          slug: null,
          name: 'Some Project',
          summary: '',
          icon_url: null,
          downloads: 0,
          author: '',
          updated_at: null,
        },
        description: '',
        website_url: null,
      },
    }),
    modsVersions: vi.fn().mockResolvedValue({
      status: 'ok',
      data: [
        {
          source: 'modrinth',
          project_id: 'pack1',
          version_id: 'v1',
          name: 'Pretty Pack 1.0',
          version_number: '1.0',
          mc_versions: ['1.20.1'],
          loaders: [],
          primary_file: {
            filename: 'pack.zip',
            url: '',
            sha1: '',
            size: 0,
            distribution_allowed: true,
          },
          deps: [],
          published_at: null,
        },
      ],
    }),
    assetInstall: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsInstallWithDeps: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
  events: {
    modInstalled: { listen: vi.fn().mockResolvedValue(() => {}) },
    modUninstalled: { listen: vi.fn().mockResolvedValue(() => {}) },
    modToggle: { listen: vi.fn().mockResolvedValue(() => {}) },
  },
}));

import ModBrowseView from '$lib/mods/ModBrowseView.svelte';
import ModDetailModal from '$lib/mods/ModDetailModal.svelte';

const tick = () => new Promise((r) => setTimeout(r, 0));

describe('ModBrowseView — content kind', () => {
  it('omits the loader filter control for resource packs', async () => {
    render(ModBrowseView, {
      props: {
        source: 'modrinth',
        instanceId: 'i',
        mcVersion: '1.20.1',
        loader: 'fabric',
        kind: 'resource_pack',
      },
    });
    // Open the Filters drawer where the loader segmented control lives.
    await fireEvent.click(screen.getByTestId('browse-filters-button'));
    // The loader facet must not render for resource packs.
    expect(screen.queryByTestId('browse-loader-segment')).toBeNull();
    expect(screen.queryByLabelText(/loader/i)).toBeNull();
    // No loader chip should surface either.
    expect(screen.queryByTestId('browse-chip-loader')).toBeNull();
  });

  it('keeps the loader filter control for mods (default kind)', async () => {
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await fireEvent.click(screen.getByTestId('browse-filters-button'));
    expect(screen.getByTestId('browse-loader-segment')).toBeTruthy();
  });

  it('threads kind into the modsSearch query and drops the loader facet for resource packs', async () => {
    const mod = await import('$lib/ipc/bindings');
    const search = mod.commands.modsSearch as unknown as ReturnType<typeof vi.fn>;
    search.mockClear();
    render(ModBrowseView, {
      props: {
        source: 'modrinth',
        instanceId: 'i',
        mcVersion: '1.20.1',
        loader: 'fabric',
        kind: 'resource_pack',
      },
    });
    // The mount-time search effect fires once filters settle.
    for (let i = 0; i < 4; i++) await tick();
    expect(search).toHaveBeenCalled();
    const arg = search.mock.calls[0]![0] as { kind: string; loader: string | null };
    expect(arg.kind).toBe('resource_pack');
    // Resource packs are loader-agnostic — no loader facet is ever sent.
    expect(arg.loader).toBeNull();
  });

  it('sends kind="mod" in the search query by default', async () => {
    const mod = await import('$lib/ipc/bindings');
    const search = mod.commands.modsSearch as unknown as ReturnType<typeof vi.fn>;
    search.mockClear();
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    for (let i = 0; i < 4; i++) await tick();
    expect(search).toHaveBeenCalled();
    const arg = search.mock.calls[0]![0] as { kind: string };
    expect(arg.kind).toBe('mod');
  });

  it('installs a shader via assetInstall, not modsInstallWithDeps', async () => {
    const mod = await import('$lib/ipc/bindings');
    const search = mod.commands.modsSearch as unknown as ReturnType<typeof vi.fn>;
    const assetInstall = mod.commands.assetInstall as unknown as ReturnType<typeof vi.fn>;
    const installWithDeps = mod.commands.modsInstallWithDeps as unknown as ReturnType<typeof vi.fn>;
    assetInstall.mockClear();
    installWithDeps.mockClear();
    // One shader result the user can click "Install" on.
    search.mockResolvedValue({
      status: 'ok',
      data: {
        hits: [
          {
            source: 'modrinth',
            project_id: 'shader1',
            slug: 'pretty-shader',
            name: 'Pretty Shader',
            summary: '',
            icon_url: null,
            downloads: 1,
            author: '',
            updated_at: null,
          },
        ],
        total: 1,
        offset: 0,
        page_size: 20,
      },
    });
    render(ModBrowseView, {
      props: {
        source: 'modrinth',
        instanceId: 'i',
        mcVersion: '1.20.1',
        loader: 'fabric',
        kind: 'shader',
      },
    });
    const installBtn = await screen.findByRole('button', { name: /^install$/i });
    await fireEvent.click(installBtn);
    for (let i = 0; i < 4; i++) await tick();
    // Shader install must take the asset path, with kind === 'shader'.
    expect(assetInstall).toHaveBeenCalled();
    expect(assetInstall.mock.calls[0]![2]).toBe('shader');
    // The dependency-aware mod path must NOT be invoked.
    expect(installWithDeps).not.toHaveBeenCalled();
  });

  // ── Detail modal loader-propagation ──────────────────────────────────────

  it('passes loader=null to ModDetailModal for shader kind so modsVersions fetches all MC-compatible versions', async () => {
    // Render the detail modal directly with loader=null (the value ModBrowseView
    // now passes for non-mod kinds). modsVersions must be called without a loader
    // filter so the user sees every MC-compatible version, not just Fabric ones.
    const mod = await import('$lib/ipc/bindings');
    const modsVersions = mod.commands.modsVersions as unknown as ReturnType<typeof vi.fn>;
    modsVersions.mockClear();

    render(ModDetailModal, {
      props: {
        source: 'modrinth',
        projectId: 'shader1',
        mcVersion: '1.20.1',
        loader: null,
        onClose: () => {},
        onInstall: () => {},
      },
    });
    // Let the async load() complete.
    for (let i = 0; i < 4; i++) await tick();

    expect(modsVersions).toHaveBeenCalled();
    // The loader argument (4th positional) must be null — no loader filter.
    const callArgs = modsVersions.mock.calls[0] as [string, string, string | null, string | null];
    expect(callArgs[3]).toBeNull();
  });

  it('passes the instance loader to ModDetailModal for mod kind so modsVersions filters by loader', async () => {
    // For mods, ModBrowseView passes loader={loader} unchanged — the existing
    // behaviour must not regress.
    const mod = await import('$lib/ipc/bindings');
    const modsVersions = mod.commands.modsVersions as unknown as ReturnType<typeof vi.fn>;
    modsVersions.mockClear();

    render(ModDetailModal, {
      props: {
        source: 'modrinth',
        projectId: 'mod1',
        mcVersion: '1.20.1',
        loader: 'fabric',
        onClose: () => {},
        onInstall: () => {},
      },
    });
    for (let i = 0; i < 4; i++) await tick();

    expect(modsVersions).toHaveBeenCalled();
    const callArgs = modsVersions.mock.calls[0] as [string, string, string | null, string | null];
    expect(callArgs[3]).toBe('fabric');
  });

  it('ModBrowseView passes loader=null to ModDetailModal when kind=shader (integration)', async () => {
    // End-to-end: ModBrowseView with kind=shader renders one hit, the user
    // opens the detail modal by clicking the card title, and modsVersions is
    // called with null (not the instance loader 'fabric') so the modal shows
    // all MC-compatible shader versions.
    const mod = await import('$lib/ipc/bindings');
    const search = mod.commands.modsSearch as unknown as ReturnType<typeof vi.fn>;
    const modsVersions = mod.commands.modsVersions as unknown as ReturnType<typeof vi.fn>;
    modsVersions.mockClear();

    search.mockResolvedValue({
      status: 'ok',
      data: {
        hits: [
          {
            source: 'modrinth',
            project_id: 'shader1',
            slug: 'pretty-shader',
            name: 'Pretty Shader',
            summary: '',
            icon_url: null,
            downloads: 1,
            author: '',
            updated_at: null,
          },
        ],
        total: 1,
        offset: 0,
        page_size: 20,
      },
    });

    render(ModBrowseView, {
      props: {
        source: 'modrinth',
        instanceId: 'i',
        mcVersion: '1.20.1',
        loader: 'fabric',
        kind: 'shader',
      },
    });

    // Wait for the search result card to render.
    const cardBtn = await screen.findByRole('button', { name: /Pretty Shader/i });
    await fireEvent.click(cardBtn);
    // Let ModDetailModal's async load() complete.
    for (let i = 0; i < 6; i++) await tick();

    // modsVersions must have been called with null loader (not 'fabric').
    const versionsCalls = modsVersions.mock.calls as Array<
      [string, string, string | null, string | null]
    >;
    const detailCall = versionsCalls.find((c) => c[1] === 'shader1');
    expect(detailCall).toBeDefined();
    expect(detailCall![3]).toBeNull();
  });
});
