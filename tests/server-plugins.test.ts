import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerCore } from '$lib/ipc/bindings';
import ServerPlugins from '$lib/servers/ServerPlugins.svelte';

// Shared mutable mock state + vi.fn handles, hoisted so the vi.mock factories
// (which run before imports) can close over them.
const {
  mockListPlugins,
  mockDeletePlugin,
  mockEnablePlugin,
  mockDisablePlugin,
  mockInstallPluginLocal,
  mockOpenPluginsFolder,
  mockOpenDialog,
  mockPushSuccess,
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
    mockListPlugins: vi.fn(),
    mockDeletePlugin: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockEnablePlugin: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockDisablePlugin: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockInstallPluginLocal: vi.fn().mockResolvedValue({ status: 'ok', data: 'cool.jar' }),
    mockOpenPluginsFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockOpenDialog: vi.fn(),
    mockPushSuccess: vi.fn(),
    serverRow,
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverListPlugins: mockListPlugins,
    serverDeletePlugin: mockDeletePlugin,
    serverEnablePlugin: mockEnablePlugin,
    serverDisablePlugin: mockDisablePlugin,
    serverInstallPluginLocal: mockInstallPluginLocal,
    serverOpenPluginsFolder: mockOpenPluginsFolder,
    // Browser-only command (used once the Add-plugins panel is opened).
    modsSearch: vi.fn().mockResolvedValue({ status: 'ok', data: { hits: [], total: 0 } }),
  },
}));

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get list() {
      return [serverRow];
    },
  },
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mockOpenDialog }));

vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: mockPushSuccess,
  pushWarning: vi.fn(),
}));

describe('ServerPlugins', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    vi.clearAllMocks();
    serverRow.running = false;
    serverRow.loader = 'paper';
    mockListPlugins.mockResolvedValue({
      status: 'ok',
      data: [
        { filename: 'worldedit.jar', disabled: false },
        { filename: 'essentials.jar.disabled', disabled: true },
      ],
    });
  });

  it('labels a set-aside plugin with the setAside badge', async () => {
    render(ServerPlugins, { serverId: 'srv-1' });
    expect(await screen.findByText('essentials.jar.disabled')).toBeTruthy();
    // Badge is lowercase "set aside"; the per-row action button is "Set aside".
    expect(screen.getByText('set aside')).toBeTruthy();
  });

  it('Restore on a disabled plugin calls serverEnablePlugin and refreshes', async () => {
    render(ServerPlugins, { serverId: 'srv-1' });
    const restore = await screen.findByTestId('server-plugin-restore');
    await fireEvent.click(restore);
    await waitFor(() =>
      expect(mockEnablePlugin).toHaveBeenCalledWith('srv-1', 'essentials.jar.disabled'),
    );
    // refresh re-queries the list (mount + after-restore).
    expect(mockListPlugins).toHaveBeenCalledTimes(2);
  });

  it('Set aside on an enabled plugin calls serverDisablePlugin and refreshes', async () => {
    render(ServerPlugins, { serverId: 'srv-1' });
    const disable = await screen.findByTestId('server-plugin-disable');
    await fireEvent.click(disable);
    await waitFor(() => expect(mockDisablePlugin).toHaveBeenCalledWith('srv-1', 'worldedit.jar'));
    expect(mockListPlugins).toHaveBeenCalledTimes(2);
  });

  it('Add plugins toggles the server plugin browser when stopped', async () => {
    render(ServerPlugins, { serverId: 'srv-1' });
    const add = await screen.findByTestId('server-plugins-add');
    expect((add as HTMLButtonElement).disabled).toBe(false);
    await fireEvent.click(add);
    expect(await screen.findByTestId('server-plugin-browser')).toBeTruthy();
  });

  it('Install .jar picks a file and calls serverInstallPluginLocal', async () => {
    mockOpenDialog.mockResolvedValue('C:/dl/cool.jar');
    render(ServerPlugins, { serverId: 'srv-1' });
    const btn = await screen.findByTestId('server-plugins-install-local');
    await fireEvent.click(btn);
    await waitFor(() =>
      expect(mockInstallPluginLocal).toHaveBeenCalledWith('srv-1', 'C:/dl/cool.jar'),
    );
    expect(mockPushSuccess).toHaveBeenCalled();
  });

  it('Install .jar is a no-op when the picker is cancelled', async () => {
    mockOpenDialog.mockResolvedValue(null);
    render(ServerPlugins, { serverId: 'srv-1' });
    const btn = await screen.findByTestId('server-plugins-install-local');
    await fireEvent.click(btn);
    await waitFor(() => expect(mockOpenDialog).toHaveBeenCalled());
    expect(mockInstallPluginLocal).not.toHaveBeenCalled();
  });

  it('disables management + shows a hint while the server runs', async () => {
    serverRow.running = true;
    render(ServerPlugins, { serverId: 'srv-1' });
    await screen.findByText('worldedit.jar');
    expect(screen.getByText('Stop the server to change its plugins.')).toBeTruthy();
    expect((screen.getByTestId('server-plugins-add') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId('server-plugins-install-local') as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it('renders only the requiresCore hint for a non-plugin core', async () => {
    serverRow.loader = 'forge';
    render(ServerPlugins, { serverId: 'srv-1' });
    expect(
      await screen.findByText(
        "Plugins need a Paper or Purpur core. You can switch this server's core in the Settings tab.",
      ),
    ).toBeTruthy();
    expect(screen.queryByTestId('server-plugins-add')).toBeNull();
    expect(screen.queryByTestId('server-plugins-install-local')).toBeNull();
    expect(screen.queryByText('worldedit.jar')).toBeNull();
  });
});
