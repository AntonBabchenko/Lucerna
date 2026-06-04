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
          loaders: [],
          game_versions: ['1.20.1'],
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
});
