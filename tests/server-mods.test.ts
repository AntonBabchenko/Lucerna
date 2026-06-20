import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { LoaderKind } from '$lib/ipc/bindings';
import ServerMods from '$lib/servers/ServerMods.svelte';

// Shared mutable mock state + vi.fn handles, hoisted so the vi.mock factories
// (which run before imports) can close over them.
const {
  mockListMods,
  mockListDatapacks,
  mockDeleteMod,
  mockEnableMod,
  mockInstallLocal,
  mockOpenFolder,
  mockQuarantine,
  mockOpenDialog,
  mockPushSuccess,
  serverRow,
} = vi.hoisted(() => {
  const serverRow = {
    id: 'srv-1',
    name: 'My Server',
    mc_version: '1.20.1',
    loader: 'forge' as LoaderKind,
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
    mockListDatapacks: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    mockDeleteMod: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockEnableMod: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockInstallLocal: vi.fn().mockResolvedValue({ status: 'ok', data: 'cool.jar' }),
    mockOpenFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    mockQuarantine: vi
      .fn()
      .mockResolvedValue({ ok: true, report: { disabled: [], kept_because_required: [] } }),
    mockOpenDialog: vi.fn(),
    mockPushSuccess: vi.fn(),
    serverRow,
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverListMods: mockListMods,
    serverListDatapacks: mockListDatapacks,
    serverDeleteMod: mockDeleteMod,
    serverEnableMod: mockEnableMod,
    serverInstallLocal: mockInstallLocal,
    serverOpenFolder: mockOpenFolder,
    // Browser-only commands (used once the Add-mods panel is opened).
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'present' }),
    modsSearch: vi.fn().mockResolvedValue({ status: 'ok', data: { hits: [], total: 0 } }),
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

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mockOpenDialog }));

vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: mockPushSuccess,
  pushWarning: vi.fn(),
}));

describe('ServerMods', () => {
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
    mockListDatapacks.mockResolvedValue({ status: 'ok', data: [] });
  });

  it('(#35) labels a set-aside jar with its quarantine reason', async () => {
    render(ServerMods, { serverId: 'srv-1' });
    expect(await screen.findByText('betterf3.jar.disabled')).toBeTruthy();
    expect(screen.getByText('set aside: client-only')).toBeTruthy();
  });

  it('(#8) Restore on a disabled jar calls serverEnableMod and refreshes', async () => {
    render(ServerMods, { serverId: 'srv-1' });
    const restore = await screen.findByTestId('server-mod-restore');
    await fireEvent.click(restore);
    await waitFor(() =>
      expect(mockEnableMod).toHaveBeenCalledWith('srv-1', 'betterf3.jar.disabled'),
    );
    // refresh re-queries the list (mount + after-restore).
    expect(mockListMods).toHaveBeenCalledTimes(2);
  });

  it('(#3) Add mods toggles the server browser when stopped + non-vanilla', async () => {
    render(ServerMods, { serverId: 'srv-1' });
    const add = await screen.findByTestId('server-mods-add');
    expect((add as HTMLButtonElement).disabled).toBe(false);
    await fireEvent.click(add);
    expect(await screen.findByTestId('server-mod-browser')).toBeTruthy();
  });

  it('(#19) Install .jar picks a file and calls serverInstallLocal', async () => {
    mockOpenDialog.mockResolvedValue('C:/dl/cool.jar');
    render(ServerMods, { serverId: 'srv-1' });
    const btn = await screen.findByTestId('server-mods-install-local');
    await fireEvent.click(btn);
    await waitFor(() => expect(mockInstallLocal).toHaveBeenCalledWith('srv-1', 'C:/dl/cool.jar'));
    expect(mockPushSuccess).toHaveBeenCalled();
  });

  it('(#19) Install .jar is a no-op when the picker is cancelled', async () => {
    mockOpenDialog.mockResolvedValue(null);
    render(ServerMods, { serverId: 'srv-1' });
    const btn = await screen.findByTestId('server-mods-install-local');
    await fireEvent.click(btn);
    await waitFor(() => expect(mockOpenDialog).toHaveBeenCalled());
    expect(mockInstallLocal).not.toHaveBeenCalled();
  });

  it('disables management + shows a hint while the server runs', async () => {
    serverRow.running = true;
    render(ServerMods, { serverId: 'srv-1' });
    await screen.findByText('jei.jar');
    expect(screen.getByText('Stop the server to change its mods and datapacks.')).toBeTruthy();
    expect((screen.getByTestId('server-mods-add') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId('server-mods-install-local') as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it('hides mod-add affordances for a vanilla server (datapacks only)', async () => {
    serverRow.loader = 'vanilla';
    render(ServerMods, { serverId: 'srv-1' });
    await screen.findByText('jei.jar');
    expect(screen.queryByTestId('server-mods-add')).toBeNull();
    expect(screen.queryByTestId('server-mods-install-local')).toBeNull();
    // Datapacks remain available regardless of loader.
    expect(screen.getByTestId('server-datapack-add')).toBeTruthy();
  });
});
