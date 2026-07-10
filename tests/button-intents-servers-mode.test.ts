// Button-intent guards (DESIGN.md §15) for the servers-mode surfaces: the
// ModeSwitcher segments, the sidebar server section's create/Start/Stop, and
// the ServersPanel empty-hero create button.
import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ServerWithStatus_Serialize } from '$lib/ipc/bindings';
import { markSeen } from '$lib/onboarding/contextual-tours';

// Heavy ServersPanel tab bodies + dialog + banner are stubbed so the panel
// mounts in happy-dom without their transitive deps (console buffer, hosting
// IPC, …). Only the empty-hero branch is under test here.
vi.mock('$lib/servers/ServerConsole.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerConnectView.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerGeneralSettings.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerSettings.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerMods.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerPlugins.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerHostingTab.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerBackupsView.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerToInstanceDialog.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerDiagnosisBanner.svelte', () => ({ default: stubComponent() }));

// A minimal no-op Svelte 5 component for child stubs (Svelte 5 mounts a
// component by CALLING it, so a plain function that renders nothing works).
function stubComponent() {
  return function noopComponent() {
    return {};
  };
}

const { serverList } = vi.hoisted(() => ({ serverList: vi.fn() }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverList,
    // ServerCreateWizard children (MemorySlider / core + loader pickers) are
    // imported at module level by ServersPanel even when the wizard is closed.
    instanceMemoryBounds: vi.fn().mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    }),
    serverCoreVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listFabricLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listQuiltLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listForgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
  events: {
    serverLogLine: { listen: vi.fn() },
    serverSpawned: { listen: vi.fn() },
    serverExited: { listen: vi.fn() },
    serverUploadProgress: { listen: vi.fn() },
  },
}));

// The wizard's import mode imports the Tauri dialog at module level; stub it
// so the module loads outside a Tauri context.
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue(null),
}));

import ModeSwitcher from '$lib/layout/ModeSwitcher.svelte';
import ServerSidebarSection from '$lib/servers/ServerSidebarSection.svelte';
import ServersPanel from '$lib/servers/ServersPanel.svelte';
import { serverState } from '$lib/servers/server-state.svelte';
import { serversUi } from '$lib/servers/servers-ui.svelte';

function makeServer(id: string, running: boolean): ServerWithStatus_Serialize {
  return {
    id,
    name: id,
    mc_version: '1.21',
    loader: 'vanilla',
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

async function load(data: ServerWithStatus_Serialize[]) {
  serverList.mockResolvedValue({ status: 'ok', data });
  await serverState.refresh();
}

beforeEach(() => {
  serversUi.setMode('client');
  serversUi.selectServer(null);
  serversUi.activeTab = 'console';
  serversUi.creating = false;
  serverList.mockReset();
  // clear() runs after the serversUi resets (they persist to localStorage)
  // and before the tour markSeen calls (clear would wipe them).
  localStorage.clear();
  // Contextual tours are not under test — keep them from mounting.
  markSeen('servers');
  markSeen('serverManage');
});

describe('ModeSwitcher — segment button intents', () => {
  it('client mode: active client segment is primary sm, inactive servers is ghost sm', async () => {
    await load([]);
    render(ModeSwitcher);
    const client = screen.getByTestId('mode-switch-client');
    const servers = screen.getByTestId('mode-switch-servers');
    expect(client).toHaveBtnVariant('primary');
    expect(client).toHaveBtnSize('sm');
    expect(servers).toHaveBtnVariant('ghost');
    expect(servers).toHaveBtnSize('sm');
  });

  it('servers mode: the variants swap, sizes stay sm', async () => {
    await load([]);
    serversUi.setMode('servers');
    render(ModeSwitcher);
    const client = screen.getByTestId('mode-switch-client');
    const servers = screen.getByTestId('mode-switch-servers');
    expect(servers).toHaveBtnVariant('primary');
    expect(servers).toHaveBtnSize('sm');
    expect(client).toHaveBtnVariant('ghost');
    expect(client).toHaveBtnSize('sm');
  });
});

describe('ServerSidebarSection — button intents', () => {
  it('create server is btn-secondary btn-xs', async () => {
    await load([]);
    render(ServerSidebarSection);
    const btn = screen.getByTestId('sidebar-create-server');
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('xs');
  });

  it('Start for a stopped selected server is btn-success btn-lg', async () => {
    await load([makeServer('a', false)]);
    serversUi.selectServer('a');
    render(ServerSidebarSection);
    const btn = screen.getByTestId('sidebar-server-start');
    expect(btn).toHaveBtnVariant('success');
    expect(btn).toHaveBtnSize('lg');
  });

  it('Stop for a running selected server is btn-danger btn-lg', async () => {
    await load([makeServer('a', true)]);
    serversUi.selectServer('a');
    render(ServerSidebarSection);
    const btn = screen.getByTestId('sidebar-server-stop');
    expect(btn).toHaveBtnVariant('danger');
    expect(btn).toHaveBtnSize('lg');
  });
});

describe('ServersPanel — empty hero button intents', () => {
  it('hero create is btn-primary btn-sm', async () => {
    await load([]);
    render(ServersPanel, {
      visible: true,
      instances: [],
      versions: [],
      onInstanceCreated: vi.fn(),
    });
    const btn = screen.getByTestId('servers-hero-create');
    expect(btn).toHaveBtnVariant('primary');
    expect(btn).toHaveBtnSize('sm');
  });
});
