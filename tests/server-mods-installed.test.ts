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

  it('bumping reloadToken re-reads the mods list', async () => {
    const { rerender } = render(ServerModsInstalled, { serverId: 'srv-1', reloadToken: 0 });
    await screen.findByText('jei.jar');
    expect(mockListEnriched).toHaveBeenCalledTimes(1);
    await rerender({ serverId: 'srv-1', reloadToken: 1 });
    await waitFor(() => expect(mockListEnriched).toHaveBeenCalledTimes(2));
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
});
