import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerCore, ServerWithStatus_Serialize } from '$lib/ipc/bindings';

// The five heavy panes are stubbed so the host mounts in happy-dom without
// their transitive deps (search IPC, card grids, …). The kind switch, sub-tab
// row, dropzone and drop-consumption under test stay real.
vi.mock('$lib/servers/mods/ServerModBrowser.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/plugins/ServerPluginBrowser.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/mods/ServerDatapacks.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/addons/ServerModsInstalled.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/addons/ServerPluginsInstalled.svelte', () => ({ default: stubComponent() }));

// A minimal no-op Svelte 5 component for child stubs (Svelte 5 mounts a
// component by CALLING it, so a plain function that renders nothing works).
function stubComponent() {
  return function noopComponent() {
    return {};
  };
}

const { serverList, serverInstallLocal, serverInstallPluginLocal, serverInstallDatapack } =
  vi.hoisted(() => ({
    serverList: vi.fn(),
    serverInstallLocal: vi.fn(),
    serverInstallPluginLocal: vi.fn(),
    serverInstallDatapack: vi.fn(),
  }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverList,
    serverInstallLocal,
    serverInstallPluginLocal,
    serverInstallDatapack,
  },
  events: {
    serverLogLine: { listen: vi.fn() },
    serverSpawned: { listen: vi.fn() },
    serverExited: { listen: vi.fn() },
    serverUploadProgress: { listen: vi.fn() },
  },
}));

// The dropzone's click path imports the Tauri dialog at module level; stub it
// so the module loads outside a Tauri context.
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue(null),
}));

import ServerAddonsTab from '$lib/servers/addons/ServerAddonsTab.svelte';
import { serverState } from '$lib/servers/server-state.svelte';
import { droppedServerContent, serverAddonsKind } from '$lib/settings/state.svelte';

function makeServer(id: string, running: boolean, loader: ServerCore): ServerWithStatus_Serialize {
  return {
    id,
    name: id,
    mc_version: '1.21',
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
  };
}

async function seed(data: ServerWithStatus_Serialize[]) {
  serverList.mockResolvedValue({ status: 'ok', data });
  await serverState.refresh();
}

describe('ServerAddonsTab', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    serverInstallLocal.mockReset();
    serverInstallPluginLocal.mockReset();
    serverInstallDatapack.mockReset();
    droppedServerContent.value = null;
    serverAddonsKind.value = null;
  });

  it('fabric server offers Mods + Datapacks kinds', async () => {
    await seed([makeServer('a', false, 'fabric')]);
    render(ServerAddonsTab, { serverId: 'a' });
    const tabs = within(screen.getByTestId('server-addons-kind-switch')).getAllByRole('tab');
    expect(tabs.map((t) => t.textContent?.trim())).toEqual(['Mods', 'Datapacks']);
  });

  it('paper server offers Plugins + Datapacks; vanilla only Datapacks (no sub-tabs)', async () => {
    await seed([makeServer('a', false, 'paper')]);
    const r = render(ServerAddonsTab, { serverId: 'a' });
    let tabs = within(screen.getByTestId('server-addons-kind-switch')).getAllByRole('tab');
    expect(tabs.map((t) => t.textContent?.trim())).toEqual(['Plugins', 'Datapacks']);
    r.unmount();
    await seed([makeServer('b', false, 'vanilla')]);
    render(ServerAddonsTab, { serverId: 'b' });
    tabs = within(screen.getByTestId('server-addons-kind-switch')).getAllByRole('tab');
    expect(tabs.map((t) => t.textContent?.trim())).toEqual(['Datapacks']);
    expect(screen.queryByTestId('server-addons-subtabs')).toBeNull();
  });

  it('kind switch resets the sub-view to browse', async () => {
    await seed([makeServer('a', false, 'fabric')]);
    render(ServerAddonsTab, { serverId: 'a' });
    const kindSwitch = () => within(screen.getByTestId('server-addons-kind-switch'));
    const subTabs = () => within(screen.getByTestId('server-addons-subtabs'));

    // Open Installed on the mods kind.
    await fireEvent.click(subTabs().getByRole('tab', { name: 'Installed' }));
    expect(subTabs().getByRole('tab', { name: 'Installed' }).getAttribute('aria-selected')).toBe(
      'true',
    );

    // Switch to Datapacks (flat view, no sub-tabs) and back to Mods:
    // the sub-view must be reset to Browse.
    await fireEvent.click(kindSwitch().getByRole('tab', { name: 'Datapacks' }));
    expect(screen.queryByTestId('server-addons-subtabs')).toBeNull();
    await fireEvent.click(kindSwitch().getByRole('tab', { name: 'Mods' }));
    await waitFor(() =>
      expect(subTabs().getByRole('tab', { name: 'Browse' }).getAttribute('aria-selected')).toBe(
        'true',
      ),
    );
  });

  it('a dropped payload for the active kind installs and clears the rune', async () => {
    await seed([makeServer('a', false, 'fabric')]);
    serverInstallLocal.mockResolvedValue({ status: 'ok', data: 'x.jar' });
    render(ServerAddonsTab, { serverId: 'a' });
    droppedServerContent.value = { kind: 'mod', paths: ['C:/x.jar'] };
    await waitFor(() => expect(serverInstallLocal).toHaveBeenCalledWith('a', 'C:/x.jar'));
    expect(droppedServerContent.value).toBeNull();
  });

  it('a dropped payload for a DIFFERENT kind is left for its own pane', async () => {
    await seed([makeServer('a', false, 'fabric')]); // active kind = mod
    render(ServerAddonsTab, { serverId: 'a' });
    droppedServerContent.value = { kind: 'datapack', paths: ['C:/pack.zip'] };
    await new Promise((r) => setTimeout(r, 50));
    expect(serverInstallDatapack).not.toHaveBeenCalled();
    // The payload stays for the pane that owns the kind.
    expect(droppedServerContent.value).toEqual({ kind: 'datapack', paths: ['C:/pack.zip'] });
  });
});
