import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerCore } from '$lib/ipc/bindings';
import ServerModsInstalled from '$lib/servers/addons/ServerModsInstalled.svelte';

// Shared mutable mock state + vi.fn handles, hoisted so the vi.mock factories
// (which run before imports) can close over them. The pane now delegates its
// list to createServerInstalledData, so we mock the *enriched* command surface
// (list + one-shot enrich backfill + batched ModSummary lookup) rather than the
// legacy serverListMods.
const {
  mockListEnriched,
  mockEnrich,
  mockProjects,
  mockDeleteMod,
  mockEnableMod,
  mockDisableMod,
  mockOpenFolder,
  mockQuarantine,
  mockCheckUpdates,
  mockUpdateOne,
  serverRow,
} = vi.hoisted(() => {
  const serverRow = {
    id: 'srv-1',
    name: 'My Server',
    mc_version: '1.20.1',
    loader: 'forge' as ServerCore,
    loader_version: '47.4.0' as string | null,
    max_heap_mb: 4096,
    extra_jvm_args: '',
    created_unix_ms: 1 as number | null,
    eula_accepted: true,
    created_from_instance: null as string | null,
    running: false,
    pid: null as number | null,
    port: null as number | null,
    upload: null,
    upload_password_set: false,
  };
  return {
    mockListEnriched: vi.fn(),
    // No loose rows are re-listed in these tests: enrich reports 0 identified.
    mockEnrich: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    mockProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    mockDeleteMod: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockEnableMod: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockDisableMod: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockOpenFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockQuarantine: vi
      .fn()
      .mockResolvedValue({ ok: true, report: { disabled: [], kept_because_required: [] } }),
    mockCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    mockUpdateOne: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { installed: [], unresolved: [] } }),
    serverRow,
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverListModsEnriched: mockListEnriched,
    serverEnrichMods: mockEnrich,
    modsProjects: mockProjects,
    serverDeleteMod: mockDeleteMod,
    serverEnableMod: mockEnableMod,
    serverDisableMod: mockDisableMod,
    serverOpenFolder: mockOpenFolder,
    serverCheckModUpdates: mockCheckUpdates,
    serverUpdateOne: mockUpdateOne,
  },
}));

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get list() {
      return [serverRow];
    },
    quarantineClientMods: mockQuarantine,
  },
}));

vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
}));

// A ServerModEntryEnriched-shaped row. Rows are left "loose" (source: null) so
// ModCard renders its degraded/manual branch (title = filename) and the summary
// lookup stays out of the picture. `on_disk_filename` carries the `.disabled`
// suffix when disabled — the name every mutation command must be called with.
const modRow = (filename: string, opts: { disabled?: boolean; reason?: string | null } = {}) => ({
  filename,
  on_disk_filename: opts.disabled ? `${filename}.disabled` : filename,
  disabled: opts.disabled ?? false,
  reason: opts.reason ?? null,
  sha1: filename,
  source: null,
  project_id: null,
  version_id: null,
  name: null,
  version_number: null,
});

describe('ServerModsInstalled', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    vi.clearAllMocks();
    serverRow.running = false;
    serverRow.loader = 'forge';
    mockEnrich.mockResolvedValue({ status: 'ok', data: 0 });
    mockProjects.mockResolvedValue({ status: 'ok', data: [] });
    mockCheckUpdates.mockResolvedValue({ status: 'ok', data: [] });
    mockUpdateOne.mockResolvedValue({ status: 'ok', data: { installed: [], unresolved: [] } });
    mockListEnriched.mockResolvedValue({
      status: 'ok',
      data: [modRow('jei.jar'), modRow('betterf3.jar', { disabled: true, reason: 'client_only' })],
    });
  });

  it('renders the enriched rows as ModCard rows, with the quarantine badge', async () => {
    render(ServerModsInstalled, { serverId: 'srv-1' });
    // Enriched `filename` is the base name (no `.disabled`), even for a
    // disabled row — the suffix lives on `on_disk_filename`.
    expect(await screen.findByText('jei.jar')).toBeTruthy();
    expect(screen.getByText('betterf3.jar')).toBeTruthy();
    expect(screen.getByText('set aside: client-only')).toBeTruthy();
  });

  it('toggling an enabled mod disables it via on-disk filename', async () => {
    mockListEnriched.mockResolvedValue({ status: 'ok', data: [modRow('jei.jar')] });
    render(ServerModsInstalled, { serverId: 'srv-1' });
    await screen.findByText('jei.jar');
    await fireEvent.click(screen.getByRole('button', { name: 'Disable' }));
    await waitFor(() => expect(mockDisableMod).toHaveBeenCalledWith('srv-1', 'jei.jar'));
    // The mutation triggers a refresh (mount + after-toggle).
    expect(mockListEnriched).toHaveBeenCalledTimes(2);
  });

  it('toggling a disabled mod enables it via the .disabled on-disk filename', async () => {
    mockListEnriched.mockResolvedValue({
      status: 'ok',
      data: [modRow('betterf3.jar', { disabled: true })],
    });
    render(ServerModsInstalled, { serverId: 'srv-1' });
    await screen.findByText('betterf3.jar');
    await fireEvent.click(screen.getByRole('button', { name: 'Enable' }));
    await waitFor(() =>
      expect(mockEnableMod).toHaveBeenCalledWith('srv-1', 'betterf3.jar.disabled'),
    );
  });

  it('uninstall opens a confirm dialog, then deletes via on-disk filename', async () => {
    mockListEnriched.mockResolvedValue({ status: 'ok', data: [modRow('jei.jar')] });
    render(ServerModsInstalled, { serverId: 'srv-1' });
    await screen.findByText('jei.jar');
    // ModCard's uninstall control (aria "Remove") — the only such button until
    // the dialog opens.
    await fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    const dialog = await screen.findByRole('dialog');
    // Confirm is scoped inside the dialog so it never collides with the row's
    // own "Remove" (uninstall) button behind the modal.
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Remove' }));
    await waitFor(() => expect(mockDeleteMod).toHaveBeenCalledWith('srv-1', 'jei.jar'));
  });

  it('uninstall on a disabled mod deletes via the .disabled on-disk filename', async () => {
    // Guards the silent-no-op footgun: a disabled row's delete must target the
    // real on-disk file (`.jar.disabled`), not the base display name.
    mockListEnriched.mockResolvedValue({
      status: 'ok',
      data: [modRow('betterf3.jar', { disabled: true })],
    });
    render(ServerModsInstalled, { serverId: 'srv-1' });
    await screen.findByText('betterf3.jar');
    await fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    const dialog = await screen.findByRole('dialog');
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Remove' }));
    await waitFor(() =>
      expect(mockDeleteMod).toHaveBeenCalledWith('srv-1', 'betterf3.jar.disabled'),
    );
  });

  it('bumping reloadToken re-reads the mods list', async () => {
    const { rerender } = render(ServerModsInstalled, { serverId: 'srv-1', reloadToken: 0 });
    await screen.findByText('jei.jar');
    expect(mockListEnriched).toHaveBeenCalledTimes(1);
    await rerender({ serverId: 'srv-1', reloadToken: 1 });
    await waitFor(() => expect(mockListEnriched).toHaveBeenCalledTimes(2));
  });

  it('checks updates, shows the badge, then applies a per-row update via sha1 + target', async () => {
    // An ENRICHED row (source + resolved summary) so ModCard's list layout
    // renders a direct "Update" button — a loose row puts Update in a context
    // menu instead. `sha1` is the identity the update commands key on.
    const enriched = {
      filename: 'jei.jar',
      on_disk_filename: 'jei.jar',
      disabled: false,
      reason: null,
      sha1: 'sha-jei',
      source: 'modrinth',
      project_id: 'jei',
      version_id: 'v1',
      name: 'JEI',
      version_number: '1.0',
    };
    // Post-update the backend swaps the jar: a NEW sha1 + version (mirrors the
    // registry row swap). The re-list after apply must surface this row and the
    // stale `update_available` entry (keyed by the OLD sha1) must be gone — so a
    // cleared badge distinguishes "entry deleted + re-rendered" from a no-op.
    const enrichedAfter = {
      filename: 'jei-2.jar',
      on_disk_filename: 'jei-2.jar',
      disabled: false,
      reason: null,
      sha1: 'sha-jei-v2',
      source: 'modrinth',
      project_id: 'jei',
      version_id: 'v2',
      name: 'JEI',
      version_number: '2.0',
    };
    mockListEnriched.mockResolvedValueOnce({ status: 'ok', data: [enriched] }); // mount
    mockListEnriched.mockResolvedValue({ status: 'ok', data: [enrichedAfter] }); // post-apply refresh
    mockProjects.mockResolvedValue({
      status: 'ok',
      data: [
        {
          source: 'modrinth',
          project_id: 'jei',
          slug: 'jei',
          name: 'JEI',
          summary: '',
          icon_url: null,
          downloads: 0,
          author: 'mezz',
          updated_at: null,
        },
      ],
    });
    const target = {
      source: 'modrinth',
      project_id: 'jei',
      version_id: 'v2',
      name: 'JEI',
      version_number: '2.0',
      mc_versions: ['1.20.1'],
      loaders: ['forge'],
      primary_file: {
        filename: 'jei-2.jar',
        url: 'https://example/jei-2.jar',
        sha1: 'bb',
        size: 1,
        distribution_allowed: true,
      },
      deps: [],
      published_at: null,
    };
    mockCheckUpdates.mockResolvedValue({
      status: 'ok',
      data: [
        {
          sha1: 'sha-jei',
          name: 'JEI',
          source: 'modrinth',
          project_id: 'jei',
          current_version_id: 'v1',
          current_version_number: '1.0',
          state: { kind: 'update_available', target },
        },
      ],
    });

    render(ServerModsInstalled, { serverId: 'srv-1' });
    await screen.findByText('JEI');

    // Before checking: no update affordance.
    expect(screen.queryByTestId('mod-update-badge')).toBeNull();

    await fireEvent.click(screen.getByTestId('server-mods-check-updates'));
    await waitFor(() => expect(mockCheckUpdates).toHaveBeenCalledWith('srv-1'));

    // The update badge appears for the row with a pending update.
    await screen.findByTestId('mod-update-badge');

    // Clicking the per-row Update applies it via sha1 + the classified target.
    await fireEvent.click(screen.getByRole('button', { name: 'Update' }));
    await waitFor(() => expect(mockUpdateOne).toHaveBeenCalledWith('srv-1', 'sha-jei', target));

    // The apply re-lists (registry swap → new sha1) and drops the stale check,
    // so the update badge clears rather than lingering on the old row.
    await waitFor(() => expect(screen.queryByTestId('mod-update-badge')).toBeNull());
    // The re-list actually happened (mount + post-apply), proving the cleared
    // badge is a real refresh, not a component that never re-rendered.
    expect(mockListEnriched).toHaveBeenCalledTimes(2);
  });

  it('shows the quarantine button for a fabric server', async () => {
    serverRow.loader = 'fabric';
    render(ServerModsInstalled, { serverId: 'srv-1' });
    await screen.findByText('jei.jar');
    const quarantine = screen.getByTestId('server-mods-quarantine');
    expect((quarantine as HTMLButtonElement).disabled).toBe(false);
  });

  it('shows only the plugin-core hint on a plugin core (paper), not the mods chrome', async () => {
    serverRow.loader = 'paper';
    render(ServerModsInstalled, { serverId: 'srv-1' });
    expect(
      await screen.findByText(
        'This core does not load mods. Paper-family servers use plugins instead: pick Plugins in the Add-ons tab.',
      ),
    ).toBeTruthy();

    // No mods-management chrome: no folder/quarantine buttons, no mods list.
    expect(screen.queryByRole('button', { name: /open folder/i })).toBeNull();
    expect(screen.queryByTestId('server-mods-quarantine')).toBeNull();
    expect(screen.queryByText('jei.jar')).toBeNull();
  });

  it('search narrows the visible rows by name', async () => {
    // beforeEach lists jei.jar (enabled) + betterf3.jar (disabled).
    render(ServerModsInstalled, { serverId: 'srv-1' });
    await screen.findByText('jei.jar');
    expect(screen.getByText('betterf3.jar')).toBeTruthy();

    await fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'jei' } });

    await waitFor(() => expect(screen.queryByText('betterf3.jar')).toBeNull());
    expect(screen.getByText('jei.jar')).toBeTruthy();
  });

  it('the Disabled view filter shows only disabled rows', async () => {
    render(ServerModsInstalled, { serverId: 'srv-1' });
    await screen.findByText('jei.jar');

    // The chip group renders role="radio"; its accessible name is "Disabled <n>".
    await fireEvent.click(screen.getByRole('radio', { name: /disabled/i }));

    await waitFor(() => expect(screen.queryByText('jei.jar')).toBeNull());
    expect(screen.getByText('betterf3.jar')).toBeTruthy();
  });
});
