import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerCore } from '$lib/ipc/bindings';
import ServerPluginsInstalled from '$lib/servers/addons/ServerPluginsInstalled.svelte';

// Shared mutable mock state + vi.fn handles, hoisted so the vi.mock factories
// (which run before imports) can close over them.
const {
  mockListPlugins,
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
    mockListPlugins: vi.fn(),
    mockDeletePlugin: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockEnablePlugin: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockDisablePlugin: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockOpenPluginsFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    serverRow,
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverListPlugins: mockListPlugins,
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

describe('ServerPluginsInstalled', () => {
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

  it('renders the entries returned by serverListPlugins, with the setAside badge', async () => {
    render(ServerPluginsInstalled, { serverId: 'srv-1' });
    expect(await screen.findByText('worldedit.jar')).toBeTruthy();
    expect(screen.getByText('essentials.jar.disabled')).toBeTruthy();
    // Badge is lowercase "set aside"; the per-row action button is "Set aside".
    expect(screen.getByText('set aside')).toBeTruthy();
  });

  it('Restore on a disabled plugin calls serverEnablePlugin and refreshes', async () => {
    render(ServerPluginsInstalled, { serverId: 'srv-1' });
    const restore = await screen.findByTestId('server-plugin-restore');
    await fireEvent.click(restore);
    await waitFor(() =>
      expect(mockEnablePlugin).toHaveBeenCalledWith('srv-1', 'essentials.jar.disabled'),
    );
    // refresh re-queries the list (mount + after-restore).
    expect(mockListPlugins).toHaveBeenCalledTimes(2);
  });

  it('Set aside on an enabled plugin calls serverDisablePlugin and refreshes', async () => {
    render(ServerPluginsInstalled, { serverId: 'srv-1' });
    const disable = await screen.findByTestId('server-plugin-disable');
    await fireEvent.click(disable);
    await waitFor(() => expect(mockDisablePlugin).toHaveBeenCalledWith('srv-1', 'worldedit.jar'));
    expect(mockListPlugins).toHaveBeenCalledTimes(2);
  });

  it('bumping reloadToken re-reads the plugins list', async () => {
    const { rerender } = render(ServerPluginsInstalled, { serverId: 'srv-1', reloadToken: 0 });
    await screen.findByText('worldedit.jar');
    expect(mockListPlugins).toHaveBeenCalledTimes(1);
    await rerender({ serverId: 'srv-1', reloadToken: 1 });
    await waitFor(() => expect(mockListPlugins).toHaveBeenCalledTimes(2));
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
});
