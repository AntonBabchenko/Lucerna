import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerCore, ServerWithStatus_Serialize } from '$lib/ipc/bindings';

// The five heavy panes are stubbed so the host mounts in happy-dom without
// their transitive deps (search IPC, card grids, …). The kind switch, sub-tab
// row, dropzone and drop-consumption under test stay real.
vi.mock('$lib/servers/mods/ServerModBrowser.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/plugins/ServerPluginBrowser.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/datapacks/ServerDatapackBrowser.svelte', () => ({
  default: stubComponent(),
}));
vi.mock('$lib/servers/datapacks/ServerDatapacksInstalled.svelte', () => ({
  default: stubComponent(),
}));
vi.mock('$lib/servers/addons/ServerModsInstalled.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/addons/ServerPluginsInstalled.svelte', () => ({ default: stubComponent() }));

// A minimal no-op Svelte 5 component for child stubs (Svelte 5 mounts a
// component by CALLING it, so a plain function that renders nothing works).
function stubComponent() {
  return function noopComponent() {
    return {};
  };
}

const {
  serverList,
  serverInstallLocal,
  serverInstallPluginLocal,
  serverInstallDatapack,
  mcVersionSupportsDatapacks,
} = vi.hoisted(() => ({
  serverList: vi.fn(),
  serverInstallLocal: vi.fn(),
  serverInstallPluginLocal: vi.fn(),
  serverInstallDatapack: vi.fn(),
  mcVersionSupportsDatapacks: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    serverList,
    serverInstallLocal,
    serverInstallPluginLocal,
    serverInstallDatapack,
    mcVersionSupportsDatapacks,
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

import { hasSeen, markSeen } from '$lib/onboarding/contextual-tours';
import ServerAddonsTab from '$lib/servers/addons/ServerAddonsTab.svelte';
import { serverState } from '$lib/servers/server-state.svelte';
import { droppedServerContent, serverAddonsKind } from '$lib/settings/state.svelte';

function makeServer(
  id: string,
  running: boolean,
  loader: ServerCore,
  mcVersion = '1.21',
): ServerWithStatus_Serialize {
  return {
    id,
    name: id,
    mc_version: mcVersion,
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
    // The tab hosts the `serverAddons` contextual tour, which auto-fires on
    // first visit. Seed it seen so its popover (and the Escape/focus handling
    // that comes with it) cannot disturb the assertions below. The two tour
    // tests at the bottom clear localStorage to opt back in.
    markSeen('serverAddons');
    serverInstallLocal.mockReset();
    serverInstallPluginLocal.mockReset();
    serverInstallDatapack.mockReset();
    // 1.21 in every seeded server below, so this default (matching the real
    // gate's answer for a modern version) keeps the pre-existing tests'
    // behaviour unchanged. The one pre-1.13 test overrides it explicitly.
    mcVersionSupportsDatapacks.mockReset();
    mcVersionSupportsDatapacks.mockResolvedValue(true);
    droppedServerContent.value = null;
    serverAddonsKind.value = null;
  });

  it('fabric server offers Mods + Datapacks kinds', async () => {
    await seed([makeServer('a', false, 'fabric')]);
    render(ServerAddonsTab, { serverId: 'a' });
    const tabs = within(screen.getByTestId('server-addons-kind-switch')).getAllByRole('tab');
    expect(tabs.map((t) => t.textContent?.trim())).toEqual(['Mods', 'Datapacks (Beta)']);
  });

  it('paper server offers Plugins + Datapacks; vanilla offers Datapacks only (both with sub-tabs)', async () => {
    await seed([makeServer('a', false, 'paper')]);
    const r = render(ServerAddonsTab, { serverId: 'a' });
    let tabs = within(screen.getByTestId('server-addons-kind-switch')).getAllByRole('tab');
    expect(tabs.map((t) => t.textContent?.trim())).toEqual(['Plugins', 'Datapacks (Beta)']);
    r.unmount();
    await seed([makeServer('b', false, 'vanilla')]);
    render(ServerAddonsTab, { serverId: 'b' });
    tabs = within(screen.getByTestId('server-addons-kind-switch')).getAllByRole('tab');
    expect(tabs.map((t) => t.textContent?.trim())).toEqual(['Datapacks (Beta)']);
    // The datapack kind now gets the same Browse/Installed sub-tab row as
    // every other kind — the old flat (no sub-tabs) datapack pane is retired.
    expect(screen.queryByTestId('server-addons-subtabs')).not.toBeNull();
  });

  it('a pre-1.13 vanilla server offers no kinds and shows the empty state', async () => {
    // The hole the 1.13 gate opens: vanilla is neither mod- nor
    // plugin-capable, so a pre-1.13 vanilla server has nothing to offer once
    // datapack is gated off too.
    mcVersionSupportsDatapacks.mockResolvedValue(false);
    await seed([makeServer('c', false, 'vanilla', '1.12.2')]);
    render(ServerAddonsTab, { serverId: 'c' });
    await waitFor(() => expect(screen.queryByTestId('server-addons-no-kinds')).not.toBeNull());
    expect(screen.queryByTestId('server-addons-kind-switch')).toBeNull();
    expect(screen.queryByTestId('server-addons-subtabs')).toBeNull();
    expect(screen.queryByTestId('server-addons-dropzone')).toBeNull();
  });

  it('kind switch resets the sub-view to browse (Datapacks included)', async () => {
    await seed([makeServer('a', false, 'fabric')]);
    render(ServerAddonsTab, { serverId: 'a' });
    const kindSwitch = () => within(screen.getByTestId('server-addons-kind-switch'));
    const subTabs = () => within(screen.getByTestId('server-addons-subtabs'));

    // Open Installed on the mods kind.
    await fireEvent.click(subTabs().getByRole('tab', { name: 'Installed' }));
    expect(subTabs().getByRole('tab', { name: 'Installed' }).getAttribute('aria-selected')).toBe(
      'true',
    );

    // Switch to Datapacks: the sub-view must be reset to Browse. Datapacks
    // now carries its own Browse/Installed sub-tabs (unlike the old flat
    // pane), so this also checks that row is still present.
    await fireEvent.click(kindSwitch().getByRole('tab', { name: 'Datapacks (Beta)' }));
    await waitFor(() =>
      expect(subTabs().getByRole('tab', { name: 'Browse' }).getAttribute('aria-selected')).toBe(
        'true',
      ),
    );

    // Open Installed again on Datapacks, then switch back to Mods: the reset
    // must fire on every kind change, not just away-from-datapack.
    await fireEvent.click(subTabs().getByRole('tab', { name: 'Installed' }));
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

  it('does not mount the tour before the datapack gate answer lands (pre-1.13 vanilla burn)', async () => {
    // Unlike every other test here, this one needs the tour UNSEEN.
    localStorage.clear();
    // '1.9' is a version no other test in this file uses, and that matters:
    // `supportsDatapacksCache` in ServerAddonsTab is module-level and survives
    // between tests in the same file. A version some earlier test already
    // answered for would make the gate resolve synchronously from cache, and
    // there would be no pending window left for this test to hold open.
    let resolveGate!: (v: boolean) => void;
    // Deliberately NOT `mockReturnValueOnce`: were the gate effect ever to run
    // twice, beforeEach's `mockResolvedValue(true)` fallback would answer the
    // second call and quietly close the very window under test.
    mcVersionSupportsDatapacks.mockReturnValue(
      new Promise<boolean>((res) => {
        resolveGate = res;
      }),
    );
    await seed([makeServer('t1', false, 'vanilla', '1.9')]);
    render(ServerAddonsTab, { serverId: 't1' });
    await tick();

    // Gate pending: `supportsDatapacks` is optimistically true, so kinds is
    // ['datapack'] and both tour anchors ARE on screen — the tour would have
    // something to point at. It must still not fire: when the real answer
    // (false) lands, kinds empties, the host block unmounts, and
    // ContextualTour's destroy path marks the tour seen. Burned forever, on
    // exactly the server where it should never have appeared.
    expect(screen.queryByTestId('server-addons-kind-switch')).not.toBeNull();
    expect(screen.queryByTestId('contextual-tour-popover')).toBeNull();

    resolveGate(false);
    await tick();
    await tick();

    expect(screen.queryByTestId('server-addons-no-kinds')).not.toBeNull();
    expect(screen.queryByTestId('contextual-tour-popover')).toBeNull();
    expect(hasSeen('serverAddons')).toBe(false);
  });

  it('mounts the tour once the gate resolves with kinds present', async () => {
    localStorage.clear();
    await seed([makeServer('t2', false, 'fabric')]);
    render(ServerAddonsTab, { serverId: 't2' });
    await tick();
    await tick();
    expect(screen.getByTestId('contextual-tour-popover')).toBeTruthy();
  });
});
