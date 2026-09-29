// A server world left with only level.dat_old refuses every data pack change
// until the server has restored level.dat (D2). The Installed pane disabled
// its toggles, trash and Vanilla Tweaks builder with the reason, but the
// tab-level "drop a .zip / click to choose" zone stayed live: the file picker
// opened and the add was then refused with a red error repeating the yellow
// note. Every add path now says why up front instead — the zone (click and
// window drop) and the catalog browser's cards and detail — the way the
// client world tab does.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerCore, ServerWithStatus_Serialize } from '$lib/ipc/bindings';

const hoisted = vi.hoisted(() => {
  // Svelte 5 mounts a component by calling it with (anchor, props); the stub
  // keeps the props so a test can read what the host handed the browser.
  const browserProps: { current: Record<string, unknown> | null } = { current: null };
  const stub = () =>
    function noopComponent() {
      return {};
    };
  const browserStub = (_anchor: unknown, props: Record<string, unknown>) => {
    browserProps.current = props;
    return {};
  };
  return { browserProps, stub, browserStub };
});

vi.mock('$lib/servers/mods/ServerModBrowser.svelte', () => ({ default: hoisted.stub() }));
vi.mock('$lib/servers/plugins/ServerPluginBrowser.svelte', () => ({ default: hoisted.stub() }));
vi.mock('$lib/servers/addons/ServerModsInstalled.svelte', () => ({ default: hoisted.stub() }));
vi.mock('$lib/servers/addons/ServerPluginsInstalled.svelte', () => ({ default: hoisted.stub() }));
vi.mock('$lib/servers/datapacks/ServerDatapackBrowser.svelte', () => ({
  default: hoisted.browserStub,
}));

const cmd = vi.hoisted(() => ({
  serverList: vi.fn(),
  serverInstallDatapack: vi.fn(),
  serverInstallLocal: vi.fn(),
  serverInstallPluginLocal: vi.fn(),
  mcVersionSupportsDatapacks: vi.fn(),
  serverListDatapacks: vi.fn(),
  serverCheckDatapackUpdates: vi.fn(),
  modsProjects: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: cmd,
  events: {
    serverLogLine: { listen: vi.fn() },
    serverSpawned: { listen: vi.fn() },
    serverExited: { listen: vi.fn() },
    serverUploadProgress: { listen: vi.fn() },
  },
}));

const dialog = vi.hoisted(() => ({ open: vi.fn() }));
vi.mock('@tauri-apps/plugin-dialog', () => dialog);

import { markSeen } from '$lib/onboarding/contextual-tours';
import ServerAddonsTab from '$lib/servers/addons/ServerAddonsTab.svelte';
import { serverState } from '$lib/servers/server-state.svelte';
import { droppedServerContent, serverAddonsKind } from '$lib/settings/state.svelte';
import { dismiss, toastList } from '$lib/toasts/toasts.svelte';

function makeServer(id: string, running: boolean, loader: ServerCore = 'vanilla') {
  return {
    id,
    name: id,
    mc_version: '1.21.1',
    loader,
    loader_version: null,
    max_heap_mb: 2048,
    extra_jvm_args: '',
    created_unix_ms: null,
    eula_accepted: true,
    created_from_instance: null,
    running,
    pid: running ? 1 : null,
    port: null,
    upload: null,
    upload_password_set: false,
    last_exit_code: null,
    diagnosis_status: 'none',
  } as ServerWithStatus_Serialize;
}
async function seed(running: boolean) {
  cmd.serverList.mockResolvedValue({ status: 'ok', data: [makeServer('s1', running)] });
  await serverState.refresh();
}
function world(level_dat: 'present' | 'only_old' | 'absent' | null) {
  cmd.serverListDatapacks.mockResolvedValue({ status: 'ok', data: { level_dat, entries: [] } });
}
const zone = () => screen.getByTestId('file-dropzone');
const ONLY_OLD = 'Start the server once to restore level.dat';

beforeAll(() => locale.set('en'));
beforeEach(() => {
  markSeen('serverAddons');
  vi.clearAllMocks();
  hoisted.browserProps.current = null;
  cmd.mcVersionSupportsDatapacks.mockResolvedValue(true);
  cmd.modsProjects.mockResolvedValue({ status: 'ok', data: [] });
  cmd.serverInstallDatapack.mockResolvedValue({ status: 'ok', data: 'p.zip' });
  dialog.open.mockResolvedValue(['C:/p.zip']);
  droppedServerContent.value = null;
  serverAddonsKind.value = null;
});

describe('ServerAddonsTab — data packs on a world with only level.dat_old', () => {
  it('the drop zone is off and says why; a click opens no picker', async () => {
    await seed(false);
    world('only_old');
    render(ServerAddonsTab, { serverId: 's1', visible: true });
    await waitFor(() => expect(zone().getAttribute('aria-disabled')).toBe('true'));
    expect(zone().textContent).toContain(ONLY_OLD);
    await fireEvent.click(zone());
    expect(dialog.open).not.toHaveBeenCalled();
    expect(cmd.serverInstallDatapack).not.toHaveBeenCalled();
  });

  it('a file dropped on the window adds nothing', async () => {
    await seed(false);
    world('only_old');
    render(ServerAddonsTab, { serverId: 's1', visible: true });
    await waitFor(() => expect(zone().getAttribute('aria-disabled')).toBe('true'));
    droppedServerContent.value = { kind: 'datapack', paths: ['C:/p.zip'] };
    await new Promise((r) => setTimeout(r, 50));
    expect(cmd.serverInstallDatapack).not.toHaveBeenCalled();
  });

  // A drop is never discarded in silence (DESIGN.md §14): the window's router cannot see the
  // world's level.dat, so the pane that can says why — in the words its drop zone shows.
  it('a file dropped on the window says why nothing was added', async () => {
    for (const x of toastList()) dismiss(x.id);
    await seed(false);
    world('only_old');
    render(ServerAddonsTab, { serverId: 's1', visible: true });
    await waitFor(() => expect(zone().getAttribute('aria-disabled')).toBe('true'));
    droppedServerContent.value = { kind: 'datapack', paths: ['C:/packs/p.zip'] };
    await waitFor(() =>
      expect(toastList().filter((x) => x.kind === 'warning')).toEqual([
        // The file on a line above the reason (`reasonLines`, plan §5d L4).
        expect.objectContaining({
          title: "1 file wasn't added",
          lines: [{ names: 'p.zip', reason: ONLY_OLD }],
        }),
      ]),
    );
    expect(cmd.serverInstallDatapack).not.toHaveBeenCalled();
    expect(droppedServerContent.value).toBeNull();
  });

  it('the catalog browser is told why it cannot add', async () => {
    await seed(false);
    world('only_old');
    render(ServerAddonsTab, { serverId: 's1', visible: true });
    await waitFor(() => expect(hoisted.browserProps.current?.blockedReason).toBe(ONLY_OLD));
  });

  it('a world with level.dat keeps every add path open', async () => {
    await seed(false);
    world('present');
    render(ServerAddonsTab, { serverId: 's1', visible: true });
    await waitFor(() => expect(cmd.serverListDatapacks).toHaveBeenCalled());
    await new Promise((r) => setTimeout(r, 20));
    expect(zone().getAttribute('aria-disabled')).toBe('false');
    expect(hoisted.browserProps.current?.blockedReason ?? null).toBeNull();
    await fireEvent.click(zone());
    await waitFor(() => expect(cmd.serverInstallDatapack).toHaveBeenCalledWith('s1', 'C:/p.zip'));
  });

  // The note tells the user to start the server once. When it stops, the
  // world has its level.dat back: the pane re-reads it, and the zone opens —
  // not only after the user leaves the tab and comes back.
  it('opens again once a server run has restored level.dat', async () => {
    await seed(true);
    world('only_old');
    render(ServerAddonsTab, { serverId: 's1', visible: true });
    await waitFor(() => expect(cmd.serverListDatapacks).toHaveBeenCalledTimes(1));
    world('present');
    await seed(false);
    await waitFor(() => expect(cmd.serverListDatapacks).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(zone().getAttribute('aria-disabled')).toBe('false'));
    expect(within(zone()).queryByText(ONLY_OLD)).toBeNull();
  });
});
