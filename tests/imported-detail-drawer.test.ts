import { fireEvent, render, waitFor, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

const openUrlMock = vi.fn().mockResolvedValue(undefined);
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: (url: string) => openUrlMock(url),
}));

// Mocks declared before SUT import so vitest hoists them ahead of
// the module graph. All commands return the tauri-specta
// `{ status, data | error }` shape — see typedError in bindings.ts.
//
// Surfaces:
//   - `modsListInstalled`: IPC the drawer hits at mount to populate
//     the Mods section.
//   - `modsProject`: per-mod project lookup used to recover display
//     names ("Xaero's Minimap" instead of "25.3.14") — mirrors the
//     same pattern as src/lib/mods/InstalledModsView.svelte.
//   - `modpackStatus`: pack-origin snapshot + live diff so the drawer
//     can badge each row by provenance and surface a Removed-from-
//     pack section. Default mock returns `{ data: null }` so tests
//     that don't care about provenance behave like a no-origin
//     instance (manually created or pre-bundle-2 import).
//   - `modpackRestoreFile`: re-install path triggered from the
//     Removed-from-pack Restore button.
//   - `deleteInstance`: fired from the confirm overlay's red button.
//
// Tests override these via `vi.mocked(...).mockResolvedValueOnce(...)`
// per-case where the defaults aren't enough.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn().mockResolvedValue({
      status: 'ok',
      data: [
        {
          filename: 'sodium.jar',
          sha1: 'a',
          name: 'Sodium',
          version_number: '0.5.8',
          source: 'modrinth',
          project_id: 'p1',
          version_id: 'v1',
          installed_at: '',
          enabled: true,
        },
        {
          filename: 'lithium.jar',
          sha1: 'b',
          name: 'Lithium',
          version_number: '0.11.2',
          source: 'modrinth',
          project_id: 'p2',
          version_id: 'v2',
          installed_at: '',
          enabled: false,
        },
      ],
    }),
    modsProject: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { summary: { name: 'Resolved Name' }, description: '', website_url: null },
    }),
    modpackStatus: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modpackRestoreFile: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        filename: 'restored.jar',
        sha1: 'restored',
        name: 'Restored',
        source: 'modrinth',
        project_id: 'pr',
        version_id: 'vr',
        version_number: '',
        installed_at: '',
        enabled: true,
      },
    }),
    deleteInstance: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modpackUpdateStatus: vi.fn().mockResolvedValue({ status: 'ok', data: { kind: 'up_to_date' } }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
  },
}));

import type { InstanceWithStatus, ModSource } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import ImportedDetailDrawer from '$lib/modpacks/ImportedDetailDrawer.svelte';

// Factory keeps the per-test setup short. Tests override mrpack_summary,
// mrpack_project_id, and mrpack_source to exercise the conditional
// rendering branches in the drawer (description, source link).
function instance(overrides: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'i1',
    name: 'My Instance',
    mc_version: '1.20.1',
    loader: 'fabric',
    loader_version: '0.15.7',
    max_heap_mb: 2048,
    extra_jvm_args: '',
    created_unix_ms: Date.now() - 86400000,
    ready: true,
    mrpack_name: 'Cool Pack',
    mrpack_version: '1.0',
    mrpack_project_id: 'ABC123',
    mrpack_source: 'modrinth' as ModSource,
    mrpack_summary: 'A pack about being cool.',
    mrpack_version_id: null,
    integrity: null,
    ...overrides,
  };
}

describe('ImportedDetailDrawer', () => {
  it('renders pack name, version, and format badge', () => {
    const { getByRole, container } = render(ImportedDetailDrawer, {
      props: {
        inst: instance(),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    const heading = getByRole('heading', { level: 3 });
    expect(heading.textContent).toContain('Cool Pack');
    expect(container.textContent).toContain('v1.0');
    expect(container.textContent).toContain('Modrinth .mrpack');
  });

  it('renders description when mrpack_summary is non-null', () => {
    const { getByTestId } = render(ImportedDetailDrawer, {
      props: {
        inst: instance(),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    expect(getByTestId('imported-detail-summary').textContent).toContain(
      'A pack about being cool.',
    );
  });

  it('hides description when mrpack_summary is null', () => {
    const { queryByTestId } = render(ImportedDetailDrawer, {
      props: {
        inst: instance({ mrpack_summary: null }),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    expect(queryByTestId('imported-detail-summary')).toBeNull();
  });

  it('renders Modrinth source link with /modpack/<id> path', async () => {
    const { getByTestId } = render(ImportedDetailDrawer, {
      props: {
        inst: instance({ mrpack_source: 'modrinth', mrpack_project_id: 'XYZ789' }),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    // Source link is now a <button> routing through the Tauri opener plugin.
    const link = getByTestId('imported-detail-source-link') as HTMLButtonElement;
    expect(link.tagName).toBe('BUTTON');
    expect(link.getAttribute('href')).toBeNull();
    expect(link.textContent).toContain('Modrinth');
    // Clicking must invoke openUrl with the correct Modrinth modpack URL.
    openUrlMock.mockClear();
    await fireEvent.click(link);
    await vi.waitFor(() => {
      expect(openUrlMock).toHaveBeenCalledWith('https://modrinth.com/modpack/XYZ789');
    });
  });

  it('renders CurseForge source link with /projects/<id> path', async () => {
    const { getByTestId } = render(ImportedDetailDrawer, {
      props: {
        inst: instance({ mrpack_source: 'curseforge', mrpack_project_id: '12345' }),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    // Source link is now a <button> routing through the Tauri opener plugin.
    const link = getByTestId('imported-detail-source-link') as HTMLButtonElement;
    expect(link.tagName).toBe('BUTTON');
    expect(link.getAttribute('href')).toBeNull();
    expect(link.textContent).toContain('CurseForge');
    // Clicking must invoke openUrl with the correct CurseForge projects URL.
    openUrlMock.mockClear();
    await fireEvent.click(link);
    await vi.waitFor(() => {
      expect(openUrlMock).toHaveBeenCalledWith('https://www.curseforge.com/projects/12345');
    });
  });

  it('renders the mod list after modsListInstalled resolves', async () => {
    const { getByTestId, findByTestId } = render(ImportedDetailDrawer, {
      props: {
        inst: instance(),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    const list = await findByTestId('imported-detail-mods-list');
    expect(list.textContent).toContain('Sodium');
    expect(list.textContent).toContain('Lithium');
    // Loading-state node is gone once the promise resolves.
    expect(() => getByTestId('imported-detail-mods-loading')).toThrow();
  });

  it('renders empty-state when modsListInstalled returns []', async () => {
    vi.mocked(commands.modsListInstalled).mockResolvedValueOnce({
      status: 'ok',
      data: [],
    });
    const { findByTestId } = render(ImportedDetailDrawer, {
      props: {
        inst: instance({ id: 'empty' }),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    const empty = await findByTestId('imported-detail-mods-empty');
    expect(empty.textContent).toContain('No mods installed');
  });

  it('Open instance click fires onOpenInstance(id)', async () => {
    const onOpenInstance = vi.fn();
    const { getByTestId } = render(ImportedDetailDrawer, {
      props: {
        inst: instance({ id: 'open-me' }),
        onClose: () => {},
        onOpenInstance,
        onDeleted: () => {},
      },
    });
    await fireEvent.click(getByTestId('imported-detail-open'));
    expect(onOpenInstance).toHaveBeenCalledWith('open-me');
  });

  it('Delete → Cancel hides the confirm overlay without calling deleteInstance', async () => {
    vi.mocked(commands.deleteInstance).mockClear();
    const { getByTestId, queryByTestId } = render(ImportedDetailDrawer, {
      props: {
        inst: instance(),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    await fireEvent.click(getByTestId('imported-detail-delete'));
    // Confirm modal is now visible.
    expect(getByTestId('imported-detail-delete-cancel')).toBeTruthy();
    await fireEvent.click(getByTestId('imported-detail-delete-cancel'));
    // Modal gone, no IPC call.
    expect(queryByTestId('imported-detail-delete-cancel')).toBeNull();
    expect(commands.deleteInstance).not.toHaveBeenCalled();
  });

  it('Delete → Confirm calls deleteInstance(id) and fires onDeleted', async () => {
    vi.mocked(commands.deleteInstance).mockClear();
    const onDeleted = vi.fn();
    const { getByTestId } = render(ImportedDetailDrawer, {
      props: {
        inst: instance({ id: 'doomed' }),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted,
      },
    });
    await fireEvent.click(getByTestId('imported-detail-delete'));
    await fireEvent.click(getByTestId('imported-detail-delete-confirm'));
    await waitFor(() => {
      expect(commands.deleteInstance).toHaveBeenCalledWith('doomed');
      expect(onDeleted).toHaveBeenCalled();
    });
  });

  // Bundle 2 — provenance badges + restore + name enrichment.

  it('recovers display name via modsProject (sub-3 workaround)', async () => {
    vi.mocked(commands.modsProject).mockResolvedValueOnce({
      status: 'ok',
      data: {
        summary: {
          source: 'modrinth',
          project_id: 'p1',
          slug: 'xaeros-minimap',
          name: "Xaero's Minimap",
          summary: '',
          icon_url: null,
          downloads: 0,
          author: '',
          updated_at: null,
        },
        body_html: '',
        gallery: [],
        website_url: null,
      },
    });
    vi.mocked(commands.modsListInstalled).mockResolvedValueOnce({
      status: 'ok',
      data: [
        {
          filename: 'minimap.jar',
          sha1: 'a',
          name: '25.3.14', // raw version-number-like display
          version_number: '25.3.14',
          source: 'modrinth',
          project_id: 'p1',
          version_id: 'v1',
          installed_at: '',
          enabled: true,
        },
      ],
    });
    const { findByTestId } = render(ImportedDetailDrawer, {
      props: {
        inst: instance(),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    const list = await findByTestId('imported-detail-mods-list');
    await waitFor(() => {
      expect(list.textContent).toContain("Xaero's Minimap");
    });
  });

  it("renders 'from pack' badge for mods present in the origin", async () => {
    vi.mocked(commands.modpackStatus).mockResolvedValueOnce({
      status: 'ok',
      data: {
        origin: {
          project_id: null,
          source: 'modrinth',
          project_name: 'Cool Pack',
          version: '1.0',
          files: [
            {
              sha1: 'a',
              name: 'Sodium',
              filename: 'sodium.jar',
              install_path: 'mods/sodium.jar',
              url: 'https://cdn',
              size: 1,
              project_id: 'p1',
              version_id: 'v1',
              env_client: 'required',
              source: 'modrinth',
            },
          ],
        },
        installed_shas: ['a'],
        removed_files: [],
        added_count: 0,
        is_modified: false,
        missing_mods: [],
      },
    });
    const { findByTestId } = render(ImportedDetailDrawer, {
      props: {
        inst: instance(),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    const badge = await findByTestId('mod-badge-pack-a');
    expect(badge.textContent).toContain('pack');
  });

  it("renders 'user-installed' badge for mods with a source but not in origin", async () => {
    vi.mocked(commands.modpackStatus).mockResolvedValueOnce({
      status: 'ok',
      data: {
        origin: {
          project_id: null,
          source: 'modrinth',
          project_name: 'Cool Pack',
          version: '1.0',
          files: [],
        },
        installed_shas: ['a', 'b'],
        removed_files: [],
        added_count: 2,
        is_modified: true,
        missing_mods: [],
      },
    });
    const { findByTestId } = render(ImportedDetailDrawer, {
      props: {
        inst: instance(),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    // mod 'a' has source 'modrinth' set in the default list mock
    const badge = await findByTestId('mod-badge-user-a');
    expect(badge.textContent).toContain('+');
  });

  it("renders 'manual' badge for mods without a source", async () => {
    vi.mocked(commands.modsListInstalled).mockResolvedValueOnce({
      status: 'ok',
      data: [
        {
          filename: 'manual.jar',
          sha1: 'm',
          name: 'manual.jar',
          version_number: null,
          source: null,
          project_id: null,
          version_id: null,
          installed_at: '',
          enabled: true,
        },
      ],
    });
    vi.mocked(commands.modpackStatus).mockResolvedValueOnce({
      status: 'ok',
      data: {
        origin: {
          project_id: null,
          source: 'modrinth',
          project_name: 'Cool Pack',
          version: '1.0',
          files: [],
        },
        installed_shas: ['m'],
        removed_files: [],
        added_count: 1,
        is_modified: true,
        missing_mods: [],
      },
    });
    const { findByTestId } = render(ImportedDetailDrawer, {
      props: {
        inst: instance(),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    const badge = await findByTestId('mod-badge-manual-m');
    expect(badge.textContent).toContain('?');
  });

  it('renders Removed-from-pack section when status.removed_files is non-empty', async () => {
    vi.mocked(commands.modpackStatus).mockResolvedValueOnce({
      status: 'ok',
      data: {
        origin: {
          project_id: null,
          source: 'modrinth',
          project_name: 'Cool Pack',
          version: '1.0',
          files: [
            {
              sha1: 'gone',
              name: 'Removed Mod',
              filename: 'removed.jar',
              install_path: 'mods/removed.jar',
              url: 'https://cdn',
              size: 1,
              project_id: 'pr',
              version_id: 'vr',
              env_client: 'required',
              source: 'modrinth',
            },
          ],
        },
        installed_shas: [],
        removed_files: [
          {
            sha1: 'gone',
            name: 'Removed Mod',
            filename: 'removed.jar',
            install_path: 'mods/removed.jar',
            url: 'https://cdn',
            size: 1,
            project_id: 'pr',
            version_id: 'vr',
            env_client: 'required',
            source: 'modrinth',
          },
        ],
        added_count: 0,
        is_modified: true,
        missing_mods: [],
      },
    });
    const { findByTestId } = render(ImportedDetailDrawer, {
      props: {
        inst: instance(),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    const section = await findByTestId('imported-detail-removed-section');
    expect(section.textContent).toContain('Removed Mod');
  });

  it('lists missing mods with three states, reason chips, source links, and correct header count', async () => {
    vi.mocked(commands.modpackStatus).mockResolvedValueOnce({
      status: 'ok',
      data: {
        origin: {
          project_id: null,
          source: 'modrinth',
          project_name: 'Cool Pack',
          version: '1.0',
          files: [],
        },
        installed_shas: [],
        removed_files: [],
        added_count: 0,
        is_modified: false,
        missing_mods: [
          {
            entry: {
              reason: 'distribution_disabled',
              mod_name: 'SRP',
              manual_action_url: 'https://www.curseforge.com/projects/1',
              filename: 'srp.jar',
              size: 1,
              sha1: 'aa',
              project_id: '1',
            },
            state: 'missing',
          },
          {
            entry: {
              reason: 'distribution_disabled',
              mod_name: 'JEI',
              manual_action_url: 'https://www.curseforge.com/projects/2',
              filename: 'jei.jar',
              size: 2,
              sha1: 'bb',
              project_id: '2',
            },
            state: 'different_version',
          },
          {
            entry: {
              reason: 'host_not_allowed',
              mod_name: 'Lib',
              manual_action_url: 'https://example.com/lib.jar',
              filename: 'lib.jar',
              size: 3,
              sha1: 'cc',
              project_id: null,
            },
            state: 'installed',
          },
        ],
      },
    });
    const { findByTestId } = render(ImportedDetailDrawer, {
      props: { inst: instance(), onClose: () => {}, onOpenInstance: () => {}, onDeleted: () => {} },
    });
    const section = await findByTestId('imported-detail-missing-section');

    // All three entries are rendered.
    expect(section.textContent).toContain('SRP');
    expect(section.textContent).toContain('JEI');
    expect(section.textContent).toContain('Lib');

    // The missing entry shows the reason chip.
    expect(section.textContent).toContain('Distribution disabled');

    // The different_version entry shows the version note.
    expect(section.textContent).toMatch(/different version/i);

    // Header count is 2 (missing + different_version, not the installed one).
    expect(section.textContent).toContain('(2)');

    // The "Open ↗" controls are now <button>s routing through the Tauri opener
    // plugin — not <a href> — to prevent javascript: injection.
    const links = within(section).getAllByText('Open') as HTMLButtonElement[];
    expect(links[0].tagName).toBe('BUTTON');
    expect(links[0].getAttribute('href')).toBeNull();
    // Clicking each button must invoke openUrl with the correct manual_action_url.
    // links[0] = SRP (state: missing), links[1] = JEI (state: different_version).
    // Lib (state: installed) has no Open ↗ button.
    openUrlMock.mockClear();
    await fireEvent.click(links[0]);
    await vi.waitFor(() => {
      expect(openUrlMock).toHaveBeenCalledWith('https://www.curseforge.com/projects/1');
    });
    openUrlMock.mockClear();
    await fireEvent.click(links[1]);
    await vi.waitFor(() => {
      expect(openUrlMock).toHaveBeenCalledWith('https://www.curseforge.com/projects/2');
    });
  });

  it('lists skipped oversized overrides as an informational section', async () => {
    vi.mocked(commands.modpackStatus).mockResolvedValueOnce({
      status: 'ok',
      data: {
        origin: {
          project_id: null,
          source: 'modrinth',
          project_name: 'All Of Create',
          version: '1.0.7',
          files: [],
          missing_mods: [],
          // 261361205 bytes = 249 MB — the real "mods.rar bundled in
          // overrides/mods/" case that used to abort the whole import.
          skipped_overrides: [{ path: 'mods/mods.rar', size: 261361205 }],
        },
        installed_shas: [],
        removed_files: [],
        added_count: 0,
        is_modified: false,
        missing_mods: [],
      },
    });
    const { findByTestId } = render(ImportedDetailDrawer, {
      props: { inst: instance(), onClose: () => {}, onOpenInstance: () => {}, onDeleted: () => {} },
    });
    const section = await findByTestId('imported-detail-skipped-section');
    expect(section.textContent).toContain('mods/mods.rar');
    expect(section.textContent).toContain('249.3 MB');
    // Heading count reflects the one skipped file.
    expect(section.textContent).toContain('(1)');
  });

  it('does not count a .zip.txt under shaderpacks/ as a shaderpack', async () => {
    vi.mocked(commands.modpackStatus).mockResolvedValueOnce({
      status: 'ok',
      data: {
        origin: {
          project_id: null,
          source: 'modrinth',
          project_name: 'All Of Create',
          version: '1.0.7',
          files: [
            {
              sha1: 's1',
              name: 'Complementary r5.7.1',
              filename: 'Complementary_r5.7.1.zip',
              install_path: 'shaderpacks/Complementary_r5.7.1.zip',
              url: '',
              size: 1000,
              project_id: '',
              version_id: '',
              env_client: 'required',
              source: 'modrinth',
            },
            {
              sha1: 's2',
              name: 'Complementary r5.6.1 note',
              filename: 'Complementary_r5.6.1.zip.txt',
              install_path: 'shaderpacks/Complementary_r5.6.1.zip.txt',
              url: '',
              size: 50,
              project_id: '',
              version_id: '',
              env_client: 'required',
              source: 'modrinth',
            },
          ],
          missing_mods: [],
          skipped_overrides: [],
        },
        installed_shas: [],
        removed_files: [],
        added_count: 0,
        is_modified: false,
        missing_mods: [],
      },
    });
    const { findByTestId } = render(ImportedDetailDrawer, {
      props: { inst: instance(), onClose: () => {}, onOpenInstance: () => {}, onDeleted: () => {} },
    });
    const shaders = await findByTestId('imported-detail-shaderpacks');
    // The real shaderpack (.zip) still renders — rows show the display name.
    expect(shaders.textContent).toContain('Complementary r5.7.1');
    // The .zip.txt download note is reclassified out of the shaderpacks list.
    expect(shaders.textContent).not.toContain('note');
    expect(shaders.textContent).not.toContain('.zip.txt');

    // The reclassified .zip.txt file lands in the Configs section instead.
    // Configs is a collapsed-by-default <details> — expand to read its body.
    const configsSection = (await findByTestId(
      'imported-detail-configs-section',
    )) as HTMLDetailsElement;
    configsSection.open = true;
    await tick();
    const configs = await findByTestId('imported-detail-configs');
    // Configs rows render the install_path.
    expect(configs.textContent).toContain('Complementary_r5.6.1.zip.txt');
  });

  it('opens attention sections and collapses inventory by default, attention first', async () => {
    vi.mocked(commands.modpackStatus).mockResolvedValueOnce({
      status: 'ok',
      data: {
        origin: {
          project_id: null,
          source: 'modrinth',
          project_name: 'P',
          version: '1',
          files: [
            {
              sha1: 'r',
              name: 'RP',
              filename: 'RP.zip',
              install_path: 'resourcepacks/RP.zip',
              url: '',
              size: 1000,
              project_id: '',
              version_id: '',
              env_client: 'required',
              source: 'modrinth',
            },
          ],
          missing_mods: [],
          skipped_overrides: [{ path: 'mods/mods.rar', size: 261361205 }],
        },
        installed_shas: [],
        removed_files: [],
        added_count: 0,
        is_modified: false,
        missing_mods: [],
      },
    });
    const { findByTestId } = render(ImportedDetailDrawer, {
      props: { inst: instance(), onClose: () => {}, onOpenInstance: () => {}, onDeleted: () => {} },
    });
    const skipped = (await findByTestId('imported-detail-skipped-section')) as HTMLDetailsElement;
    const rp = (await findByTestId('imported-detail-resourcepacks-section')) as HTMLDetailsElement;
    // Attention open, inventory collapsed.
    expect(skipped.open).toBe(true);
    expect(rp.open).toBe(false);
    // Attention renders before inventory in document order.
    expect(skipped.compareDocumentPosition(rp) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // Asset size is shown (expand to read the collapsed body).
    rp.open = true;
    await tick();
    expect((await findByTestId('imported-detail-resourcepacks')).textContent).toMatch(
      /\d+\s?(B|KB|MB)/,
    );
  });

  afterEach(async () => {
    const { drawerCache } = await import('$lib/modpacks/drawer-cache');
    drawerCache.clear();
  });

  it('renders cached mods immediately on reopen, with no Loading state', async () => {
    const { drawerCache } = await import('$lib/modpacks/drawer-cache');
    drawerCache.set('i1', {
      mods: [
        {
          filename: 'cached.jar',
          sha1: 'c1',
          name: 'Cached Mod',
          version_number: null,
          source: null,
          project_id: null,
          version_id: null,
          installed_at: '',
          enabled: true,
        },
      ],
      status: null,
      nameMap: new Map(),
    });
    const { getByTestId, queryByTestId } = render(ImportedDetailDrawer, {
      props: {
        inst: instance({ id: 'i1' }),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    // Seeded synchronously from the cache — no "Loading…" placeholder.
    expect(queryByTestId('imported-detail-mods-loading')).toBeNull();
    expect(getByTestId('imported-detail-mods-list').textContent).toContain('Cached Mod');
  });

  it('Restore button calls modpackRestoreFile with the right sha', async () => {
    vi.mocked(commands.modpackRestoreFile).mockClear();
    vi.mocked(commands.modpackStatus).mockResolvedValueOnce({
      status: 'ok',
      data: {
        origin: {
          project_id: null,
          source: 'modrinth',
          project_name: 'Cool Pack',
          version: '1.0',
          files: [
            {
              sha1: 'gone-sha',
              name: 'Gone',
              filename: 'gone.jar',
              install_path: 'mods/gone.jar',
              url: 'https://cdn',
              size: 1,
              project_id: 'pg',
              version_id: 'vg',
              env_client: 'required',
              source: 'modrinth',
            },
          ],
        },
        installed_shas: [],
        removed_files: [
          {
            sha1: 'gone-sha',
            name: 'Gone',
            filename: 'gone.jar',
            install_path: 'mods/gone.jar',
            url: 'https://cdn',
            size: 1,
            project_id: 'pg',
            version_id: 'vg',
            env_client: 'required',
            source: 'modrinth',
          },
        ],
        added_count: 0,
        is_modified: true,
        missing_mods: [],
      },
    });
    const { findByTestId } = render(ImportedDetailDrawer, {
      props: {
        inst: instance({ id: 'inst-x' }),
        onClose: () => {},
        onOpenInstance: () => {},
        onDeleted: () => {},
      },
    });
    const btn = await findByTestId('imported-detail-restore-gone-sha');
    await fireEvent.click(btn);
    await waitFor(() => {
      expect(commands.modpackRestoreFile).toHaveBeenCalledWith('inst-x', 'gone-sha');
    });
  });
});
