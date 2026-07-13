import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerCore } from '$lib/ipc/bindings';
import ServerPluginsInstalled from '$lib/servers/addons/ServerPluginsInstalled.svelte';

// Shared mutable mock state + vi.fn handles, hoisted so the vi.mock factories
// (which run before imports) can close over them. The pane now delegates its
// list to createServerInstalledData, so we mock the *enriched* command surface
// (list + enrich backfill + batched ModSummary lookup), not serverListPlugins.
const {
  mockListEnriched,
  mockEnrich,
  mockProjects,
  mockDeletePlugin,
  mockEnablePlugin,
  mockDisablePlugin,
  mockOpenPluginsFolder,
  serverRow,
} = vi.hoisted(() => {
  const serverRow = {
    id: 'srv-1',
    name: 'My Server',
    mc_version: '1.20.1',
    loader: 'paper' as ServerCore,
    loader_version: '196' as string | null,
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
    mockEnrich: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    mockProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    mockDeletePlugin: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockEnablePlugin: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockDisablePlugin: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockOpenPluginsFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    serverRow,
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverListPluginsEnriched: mockListEnriched,
    serverEnrichPlugins: mockEnrich,
    modsProjects: mockProjects,
    serverDeletePlugin: mockDeletePlugin,
    serverEnablePlugin: mockEnablePlugin,
    serverDisablePlugin: mockDisablePlugin,
    serverOpenPluginsFolder: mockOpenPluginsFolder,
  },
}));

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get list() {
      return [serverRow];
    },
  },
}));

// A ServerPluginEntryEnriched-shaped row (no quarantine reason — plugins carry
// no client/server ambiguity). Left "loose" (source: null) so ModCard renders
// its degraded/manual branch (title = filename). `on_disk_filename` carries the
// `.disabled` suffix when disabled — what every mutation command must receive.
const pluginRow = (filename: string, opts: { disabled?: boolean } = {}) => ({
  filename,
  on_disk_filename: opts.disabled ? `${filename}.disabled` : filename,
  disabled: opts.disabled ?? false,
  sha1: filename,
  source: null,
  project_id: null,
  version_id: null,
  name: null,
  version_number: null,
});

describe('ServerPluginsInstalled', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    vi.clearAllMocks();
    serverRow.running = false;
    serverRow.loader = 'paper';
    mockEnrich.mockResolvedValue({ status: 'ok', data: 0 });
    mockProjects.mockResolvedValue({ status: 'ok', data: [] });
    mockListEnriched.mockResolvedValue({
      status: 'ok',
      data: [pluginRow('worldedit.jar'), pluginRow('essentials.jar', { disabled: true })],
    });
  });

  it('renders the enriched plugins as ModCard rows', async () => {
    render(ServerPluginsInstalled, { serverId: 'srv-1' });
    expect(await screen.findByText('worldedit.jar')).toBeTruthy();
    expect(screen.getByText('essentials.jar')).toBeTruthy();
  });

  it('toggling an enabled plugin disables it via on-disk filename', async () => {
    mockListEnriched.mockResolvedValue({ status: 'ok', data: [pluginRow('worldedit.jar')] });
    render(ServerPluginsInstalled, { serverId: 'srv-1' });
    await screen.findByText('worldedit.jar');
    await fireEvent.click(screen.getByRole('button', { name: 'Disable' }));
    await waitFor(() => expect(mockDisablePlugin).toHaveBeenCalledWith('srv-1', 'worldedit.jar'));
    expect(mockListEnriched).toHaveBeenCalledTimes(2);
  });

  it('toggling a disabled plugin enables it via the .disabled on-disk filename', async () => {
    mockListEnriched.mockResolvedValue({
      status: 'ok',
      data: [pluginRow('essentials.jar', { disabled: true })],
    });
    render(ServerPluginsInstalled, { serverId: 'srv-1' });
    await screen.findByText('essentials.jar');
    await fireEvent.click(screen.getByRole('button', { name: 'Enable' }));
    await waitFor(() =>
      expect(mockEnablePlugin).toHaveBeenCalledWith('srv-1', 'essentials.jar.disabled'),
    );
  });

  it('uninstall opens a confirm dialog, then deletes via on-disk filename', async () => {
    mockListEnriched.mockResolvedValue({ status: 'ok', data: [pluginRow('worldedit.jar')] });
    render(ServerPluginsInstalled, { serverId: 'srv-1' });
    await screen.findByText('worldedit.jar');
    await fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    const dialog = await screen.findByRole('dialog');
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Remove' }));
    await waitFor(() => expect(mockDeletePlugin).toHaveBeenCalledWith('srv-1', 'worldedit.jar'));
  });

  it('uninstall on a disabled plugin deletes via the .disabled on-disk filename', async () => {
    // Guards the silent-no-op footgun: a disabled row's delete must target the
    // real on-disk file (`.jar.disabled`), not the base display name.
    mockListEnriched.mockResolvedValue({
      status: 'ok',
      data: [pluginRow('essentials.jar', { disabled: true })],
    });
    render(ServerPluginsInstalled, { serverId: 'srv-1' });
    await screen.findByText('essentials.jar');
    await fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    const dialog = await screen.findByRole('dialog');
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Remove' }));
    await waitFor(() =>
      expect(mockDeletePlugin).toHaveBeenCalledWith('srv-1', 'essentials.jar.disabled'),
    );
  });

  it('bumping reloadToken re-reads the plugins list', async () => {
    const { rerender } = render(ServerPluginsInstalled, { serverId: 'srv-1', reloadToken: 0 });
    await screen.findByText('worldedit.jar');
    expect(mockListEnriched).toHaveBeenCalledTimes(1);
    await rerender({ serverId: 'srv-1', reloadToken: 1 });
    await waitFor(() => expect(mockListEnriched).toHaveBeenCalledTimes(2));
  });

  it('renders only the requiresCore hint for a non-plugin core (fabric)', async () => {
    serverRow.loader = 'fabric';
    render(ServerPluginsInstalled, { serverId: 'srv-1' });
    expect(
      await screen.findByText(
        "Plugins need a Paper or Purpur core. You can switch this server's core in the Settings tab.",
      ),
    ).toBeTruthy();
    expect(screen.queryByRole('button', { name: /open folder/i })).toBeNull();
    expect(screen.queryByText('worldedit.jar')).toBeNull();
  });

  it('search narrows the visible rows by name', async () => {
    // beforeEach lists worldedit.jar (enabled) + essentials.jar (disabled).
    render(ServerPluginsInstalled, { serverId: 'srv-1' });
    await screen.findByText('worldedit.jar');
    expect(screen.getByText('essentials.jar')).toBeTruthy();

    await fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'world' } });

    await waitFor(() => expect(screen.queryByText('essentials.jar')).toBeNull());
    expect(screen.getByText('worldedit.jar')).toBeTruthy();
  });

  it('the Disabled view filter shows only disabled rows', async () => {
    render(ServerPluginsInstalled, { serverId: 'srv-1' });
    await screen.findByText('worldedit.jar');

    // The chip group renders role="radio"; its accessible name is "Disabled <n>".
    await fireEvent.click(screen.getByRole('radio', { name: /disabled/i }));

    await waitFor(() => expect(screen.queryByText('worldedit.jar')).toBeNull());
    expect(screen.getByText('essentials.jar')).toBeTruthy();
  });
});
