import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

// The mock has to be declared before the SUT import so vitest hoists it
// ahead of the module graph. Both commands return the tauri-specta
// `{ status, data | error }` shape — see typedError in bindings.ts.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsSearch: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { hits: [], total: 0, offset: 0, page_size: 20 } }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // refreshInstalled enriches each installed entry by fetching the
    // project's display name (so cross-platform matching works).
    // Default mock returns the same name for any lookup; specific tests
    // override per call as needed.
    modsProject: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        summary: {
          source: 'modrinth',
          project_id: 'p',
          slug: null,
          name: 'Some Mod',
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
  },
}));

import ModBrowseView from '$lib/mods/ModBrowseView.svelte';

describe('ModBrowseView', () => {
  it('pre-fills MC and Loader filters from props', () => {
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    expect(screen.getByDisplayValue('1.20.1')).toBeTruthy();
  });

  it('marks a Modrinth-installed mod as installed when viewing the CurseForge entry', async () => {
    const mod = await import('$lib/ipc/bindings');

    // Modrinth-side search returns one hit for "Cloth Config API".
    // The user installs it from Modrinth, then switches the dropdown
    // to CurseForge. CF search returns its own entry for the same
    // mod — a different project_id but the same human-readable name.
    // The card should show the Installed pill, not an active Install
    // button.
    (mod.commands.modsSearch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: {
        hits: [
          {
            source: 'curseforge',
            project_id: '348521',
            slug: 'cloth-config',
            name: 'Cloth Config API',
            summary: 'A config screen library',
            icon_url: null,
            downloads: 999,
            author: 'shedaniel',
            updated_at: null,
          },
        ],
        total: 1,
        offset: 0,
        page_size: 20,
      },
    });

    // installed-mods.json holds the entry written by install_one. The
    // crucial detail: `name` is `version.name` — the RELEASE TITLE from
    // Modrinth, not the project name. For Cloth Config API a typical
    // value is "v13.0.121 for Forge 1.20.4" or similar. The cross-
    // platform match has to find this entry despite the name mismatch
    // against the CF card's "Cloth Config API".
    (mod.commands.modsListInstalled as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: [
        {
          filename: 'cloth-config-13.0.121-forge.jar',
          sha1: 'abc',
          source: 'modrinth',
          project_id: '9s6osm5g',
          version_id: 'vx',
          name: 'v13.0.121 for Forge 1.20.4',
          version_number: '13.0.121',
          installed_at: '2026-05-19T00:00:00Z',
          enabled: true,
        },
      ],
    });

    // refreshInstalled enriches via modsProject to recover the project's
    // display name. Make the lookup return "Cloth Config API" for the
    // Modrinth project id so installedFor's cross-platform name match
    // finds it.
    (mod.commands.modsProject as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: {
        summary: {
          source: 'modrinth',
          project_id: '9s6osm5g',
          slug: 'cloth-config',
          name: 'Cloth Config API',
          summary: '',
          icon_url: null,
          downloads: 0,
          author: '',
          updated_at: null,
        },
        description: '',
        website_url: null,
      },
    });

    render(ModBrowseView, {
      props: {
        source: 'curseforge',
        instanceId: 'i',
        mcVersion: '1.20.4',
        loader: 'forge',
      },
    });
    // Three yields: refreshCfKey → modsListInstalled → search and
    // render. Then the project-name lookups (if any) settle.
    for (let i = 0; i < 4; i++) {
      await new Promise((r) => setTimeout(r, 0));
    }

    // Acceptance: the rendered card for the CF entry should show the
    // Installed pill and NOT an active Install button — because the
    // same mod is already on disk via Modrinth.
    expect(screen.queryByRole('button', { name: /^install$/i })).toBeNull();
    expect(screen.getByText(/Installed/)).toBeTruthy();
  });

  it('matches across platforms when CF labels the project with " (Forge)" suffix', async () => {
    const mod = await import('$lib/ipc/bindings');

    // User installed Cloth Config from CurseForge. CF's project.summary.name
    // for this kind of mod often carries a loader suffix like
    // "Cloth Config (Forge)" or "Cloth Config API (Fabric)" — what we
    // saw in production. The Modrinth card calls the same mod
    // "Cloth Config API". A strict equality check on the project name
    // would miss this; the match has to ignore platform-specific
    // suffixes.
    (mod.commands.modsSearch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: {
        hits: [
          {
            source: 'modrinth',
            project_id: '9s6osm5g',
            slug: 'cloth-config-api',
            name: 'Cloth Config API',
            summary: '',
            icon_url: null,
            downloads: 0,
            author: '',
            updated_at: null,
          },
        ],
        total: 1,
        offset: 0,
        page_size: 20,
      },
    });

    (mod.commands.modsListInstalled as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: [
        {
          filename: 'cloth-config-10.1.135-forge.jar',
          sha1: 'cf1',
          source: 'curseforge',
          project_id: '348521',
          version_id: 'fid',
          name: '10.1.135+forge',
          version_number: '10.1.135+forge',
          installed_at: '2026-05-19T00:00:00Z',
          enabled: true,
        },
      ],
    });

    // CF's project.summary.name for Cloth Config is the loader-suffixed
    // variant. The matcher must reduce this to the same key as the
    // Modrinth card's "Cloth Config API".
    (mod.commands.modsProject as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: {
        summary: {
          source: 'curseforge',
          project_id: '348521',
          slug: 'cloth-config',
          name: 'Cloth Config API (Forge)',
          summary: '',
          icon_url: null,
          downloads: 0,
          author: '',
          updated_at: null,
        },
        description: '',
        website_url: null,
      },
    });

    render(ModBrowseView, {
      props: {
        source: 'modrinth',
        instanceId: 'i',
        mcVersion: '1.20.4',
        loader: 'forge',
      },
    });
    for (let i = 0; i < 4; i++) {
      await new Promise((r) => setTimeout(r, 0));
    }

    expect(screen.queryByRole('button', { name: /^install$/i })).toBeNull();
    expect(screen.getByText(/Installed/)).toBeTruthy();
  });

  it('shows CurseForge banner when source is curseforge and key is missing', async () => {
    const mod = await import('$lib/ipc/bindings');
    // Use the persistent override (not `Once`) — the mount-time effect
    // chain reads the key status and the rest of the test only checks
    // the final UI state, so any subsequent reads should also see
    // 'missing'.
    (
      mod.commands.modsGetCurseforgeKeyStatus as unknown as ReturnType<typeof vi.fn>
    ).mockResolvedValue({ status: 'ok', data: 'missing' });
    render(ModBrowseView, {
      props: { source: 'curseforge', instanceId: null, mcVersion: null, loader: null },
    });
    // Two yields: first lets refreshCfKey's await resolve, second lets
    // the resulting Svelte rerender flush before we query the DOM.
    await new Promise((r) => setTimeout(r, 0));
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText(/CurseForge requires an API key/i)).toBeTruthy();
    // Restore default for any subsequent test in this file.
    (
      mod.commands.modsGetCurseforgeKeyStatus as unknown as ReturnType<typeof vi.fn>
    ).mockResolvedValue({ status: 'ok', data: 'set' });
  });

  it('advances past an all-installed first page when Show installed is unchecked', async () => {
    const mod = await import('$lib/ipc/bindings');
    // Page 1 (offset 0): 20 hits, all already installed.
    // Page 2 (offset 20): one non-installed hit. total = 21.
    const installedHit = (n: number) => ({
      source: 'modrinth' as const,
      project_id: `inst${n}`,
      slug: `inst${n}`,
      name: `Installed Mod ${n}`,
      summary: '',
      icon_url: null,
      downloads: 1000 - n,
      author: '',
      updated_at: null,
    });
    const freshHit = {
      source: 'modrinth' as const,
      project_id: 'fresh1',
      slug: 'fresh1',
      name: 'Fresh Mod',
      summary: '',
      icon_url: null,
      downloads: 500,
      author: '',
      updated_at: null,
    };
    (mod.commands.modsSearch as ReturnType<typeof vi.fn>).mockImplementation(
      (q: { offset: number }) =>
        Promise.resolve({
          status: 'ok',
          data: {
            hits:
              q.offset === 0 ? Array.from({ length: 20 }, (_, i) => installedHit(i)) : [freshHit],
            total: 21,
            offset: q.offset,
            page_size: 20,
          },
        }),
    );
    (mod.commands.modsListInstalled as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: Array.from({ length: 20 }, (_, i) => ({
        filename: `inst${i}.jar`,
        sha1: `s${i}`,
        source: 'modrinth',
        project_id: `inst${i}`,
        version_id: `v${i}`,
        name: `Installed Mod ${i}`,
        version_number: '1.0',
        installed_at: '2026-05-20T00:00:00Z',
        enabled: true,
      })),
    });
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    // Wait for the mount-time refreshInstalled effect (modsListInstalled +
    // 20×modsProject via Promise.all) to settle before toggling, so that
    // installedMods is already populated when fill(1) runs its filter.
    for (let i = 0; i < 5; i++) {
      await new Promise((r) => setTimeout(r, 0));
    }
    // Uncheck "Show installed" once the initial render has settled.
    const toggle = screen.getByRole('checkbox', { name: 'Show installed' });
    await fireEvent.click(toggle);
    // The non-installed page-2 mod is reached and rendered; the old
    // "already installed — navigate to a different page" dead end is gone.
    expect(await screen.findByText('Fresh Mod')).toBeTruthy();
    expect(screen.queryByText(/already installed/i)).toBeNull();
  });

  it('shows "of Y" only when Show installed is checked', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsSearch as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: {
        hits: [
          {
            source: 'modrinth',
            project_id: 'm1',
            slug: 'm1',
            name: 'Mod One',
            summary: '',
            icon_url: null,
            downloads: 9,
            author: '',
            updated_at: null,
          },
        ],
        total: 1,
        offset: 0,
        page_size: 20,
      },
    });
    (mod.commands.modsListInstalled as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: [],
    });
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    // Checked (default): counter carries "of 1".
    expect(await screen.findByText(/Page 1 of 1/)).toBeTruthy();
    // Unchecked: counter is "Page 1" with no total.
    const toggle = screen.getByRole('checkbox', { name: 'Show installed' });
    await fireEvent.click(toggle);
    expect(await screen.findByText('Page 1')).toBeTruthy();
    expect(screen.queryByText(/of 1/)).toBeNull();
  });

  it('pages forward and back through the buffer', async () => {
    const mod = await import('$lib/ipc/bindings');
    const hit = (id: string, name: string) => ({
      source: 'modrinth' as const,
      project_id: id,
      slug: id,
      name,
      summary: '',
      icon_url: null,
      downloads: 1,
      author: '',
      updated_at: null,
    });
    // Two full platform pages, total = 40.
    (mod.commands.modsSearch as ReturnType<typeof vi.fn>).mockImplementation(
      (q: { offset: number }) =>
        Promise.resolve({
          status: 'ok',
          data: {
            hits:
              q.offset === 0
                ? Array.from({ length: 20 }, (_, i) => hit(`a${i}`, `Alpha ${i}`))
                : Array.from({ length: 20 }, (_, i) => hit(`b${i}`, `Beta ${i}`)),
            total: 40,
            offset: q.offset,
            page_size: 20,
          },
        }),
    );
    (mod.commands.modsListInstalled as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: [],
    });
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    expect(await screen.findByText('Alpha 0')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: /Next/ }));
    expect(await screen.findByText('Beta 0')).toBeTruthy();
    expect(screen.queryByText('Alpha 0')).toBeNull();
    // Next is now disabled — the platform is exhausted (40 of 40).
    expect(screen.getByRole('button', { name: /Next/ }).hasAttribute('disabled')).toBe(true);
    await fireEvent.click(screen.getByRole('button', { name: /Prev/ }));
    expect(await screen.findByText('Alpha 0')).toBeTruthy();
  });

  it('shows "No results" for an empty search', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsSearch as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: { hits: [], total: 0, offset: 0, page_size: 20 },
    });
    (mod.commands.modsListInstalled as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: [],
    });
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    expect(await screen.findByText('No results.')).toBeTruthy();
  });
});
