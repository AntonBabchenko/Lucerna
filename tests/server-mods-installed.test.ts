import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerCore } from '$lib/ipc/bindings';
import ServerModsInstalled from '$lib/servers/addons/ServerModsInstalled.svelte';

// Shared mutable mock state + vi.fn handles, hoisted so the vi.mock factories
// (which run before imports) can close over them.
const { mockListMods, mockDeleteMod, mockEnableMod, mockOpenFolder, mockQuarantine, serverRow } =
  vi.hoisted(() => {
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
      mockListMods: vi.fn(),
      mockDeleteMod: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
      mockEnableMod: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
      mockOpenFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
      mockQuarantine: vi
        .fn()
        .mockResolvedValue({ ok: true, report: { disabled: [], kept_because_required: [] } }),
      serverRow,
    };
  });

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverListMods: mockListMods,
    serverDeleteMod: mockDeleteMod,
    serverEnableMod: mockEnableMod,
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

describe('ServerModsInstalled', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    vi.clearAllMocks();
    serverRow.running = false;
    serverRow.loader = 'forge';
    mockListMods.mockResolvedValue({
      status: 'ok',
      data: [
        { filename: 'jei.jar', disabled: false, reason: null },
        { filename: 'betterf3.jar.disabled', disabled: true, reason: 'client_only' },
      ],
    });
  });

  it('renders the entries returned by serverListMods, with quarantine badges', async () => {
    render(ServerModsInstalled, { serverId: 'srv-1' });
    expect(await screen.findByText('jei.jar')).toBeTruthy();
    expect(screen.getByText('betterf3.jar.disabled')).toBeTruthy();
    expect(screen.getByText('set aside: client-only')).toBeTruthy();
  });

  it('Restore on a disabled jar calls serverEnableMod and refreshes', async () => {
    render(ServerModsInstalled, { serverId: 'srv-1' });
    const restore = await screen.findByTestId('server-mod-restore');
    await fireEvent.click(restore);
    await waitFor(() =>
      expect(mockEnableMod).toHaveBeenCalledWith('srv-1', 'betterf3.jar.disabled'),
    );
    // refresh re-queries the list (mount + after-restore).
    expect(mockListMods).toHaveBeenCalledTimes(2);
  });

  it('bumping reloadToken re-reads the mods list', async () => {
    const { rerender } = render(ServerModsInstalled, { serverId: 'srv-1', reloadToken: 0 });
    await screen.findByText('jei.jar');
    expect(mockListMods).toHaveBeenCalledTimes(1);
    await rerender({ serverId: 'srv-1', reloadToken: 1 });
    await waitFor(() => expect(mockListMods).toHaveBeenCalledTimes(2));
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
