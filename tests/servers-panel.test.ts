import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerWithStatus_Serialize } from '$lib/ipc/bindings';
import { markSeen, storageKey } from '$lib/onboarding/contextual-tours';

// Heavy tab bodies + dialog + banner are stubbed so the panel mounts in
// happy-dom without their transitive deps (console buffer, hosting IPC, …).
// The header, tab BAR, hero and wizard branches under test stay real.
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

const { serverList, serverStart, serverStop, serverRestart, serverDiagnose } = vi.hoisted(() => ({
  serverList: vi.fn(),
  serverStart: vi.fn(),
  serverStop: vi.fn(),
  serverRestart: vi.fn(),
  serverDiagnose: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverList,
    serverStart,
    serverStop,
    serverRestart,
    serverDiagnose,
    // ServerCreateWizard children (MemorySlider / core + loader pickers).
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

function baseProps(overrides: Record<string, unknown> = {}) {
  return {
    visible: true,
    instances: [],
    versions: [],
    onInstanceCreated: vi.fn(),
    ...overrides,
  };
}

describe('ServersPanel', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    serversUi.selectServer(null);
    serversUi.activeTab = 'console';
    serversUi.creating = false;
    serverList.mockReset();
    serverStart.mockReset();
    serverStop.mockReset();
    serverRestart.mockReset();
    serverDiagnose.mockReset();
    serverDiagnose.mockResolvedValue({ status: 'error', error: { kind: 'x' } });
    // clear() runs after the serversUi resets (selectServer persists to
    // localStorage) and before the tour markSeen calls (clear would wipe them).
    localStorage.clear();
    // Suppress both contextual tours by default; the tour-visibility tests
    // re-arm the servers tour explicitly.
    markSeen('servers');
    markSeen('serverManage');
  });

  // MUST STAY FIRST in this file: it pins that the servers tour does NOT
  // mount before the first refresh() settles. listLoadedOnce is
  // module-singleton state — false only until any test in this file calls
  // load() — and vitest gives each test FILE a fresh module graph.
  it('arms the servers tour only after the first list fetch settles', async () => {
    localStorage.removeItem(storageKey('servers')); // re-arm the tour
    // No refresh() yet: the list is empty because it was never FETCHED. The
    // tour must not mount here — the spinner branch replacing it would burn
    // it as soft-skipped (ContextualTour marks itself seen on destroy).
    render(ServersPanel, baseProps());
    expect(screen.getByTestId('servers-empty-hero')).toBeTruthy();
    expect(screen.queryByTestId('contextual-tour-popover')).toBeNull();

    // First fetch settles (still empty) → the tour is now allowed to fire.
    await load([]);
    expect(await screen.findByTestId('contextual-tour-popover')).toBeTruthy();
  });

  it('shows the empty hero when there are no servers; create opens the wizard flag', async () => {
    await load([]);
    render(ServersPanel, baseProps());
    expect(screen.getByTestId('servers-empty-hero')).toBeTruthy();
    await fireEvent.click(screen.getByTestId('servers-hero-create'));
    expect(serversUi.creating).toBe(true);
  });

  it('renders the create wizard instead of the hero while creating', async () => {
    await load([]);
    serversUi.creating = true;
    render(ServersPanel, baseProps());
    expect(screen.queryByTestId('servers-empty-hero')).toBeNull();
    expect(screen.getByRole('heading', { name: 'New server' })).toBeTruthy();
  });

  it('renders the 8-tab bar for a selected server and flips the shared activeTab', async () => {
    await load([makeServer('a', false)]);
    serversUi.selectServer('a');
    render(ServersPanel, baseProps());
    expect(screen.getAllByRole('tab')).toHaveLength(8);
    await fireEvent.click(screen.getByRole('tab', { name: 'General' }));
    expect(serversUi.activeTab).toBe('general');
  });

  it('fires the servers tour on the empty panel when visible', async () => {
    await load([]);
    localStorage.removeItem(storageKey('servers')); // re-arm the tour
    render(ServersPanel, baseProps());
    expect(await screen.findByTestId('contextual-tour-popover')).toBeTruthy();
  });

  it('does not fire the servers tour while the panel is hidden (visible=false)', async () => {
    await load([]);
    localStorage.removeItem(storageKey('servers')); // re-arm the tour
    render(ServersPanel, baseProps({ visible: false }));
    expect(screen.queryByTestId('contextual-tour-popover')).toBeNull();
  });

  it('running server: header offers Restart only — Start/Stop moved to the sidebar', async () => {
    await load([makeServer('a', true)]);
    serversUi.selectServer('a');
    render(ServersPanel, baseProps());
    expect(screen.getByText('Restart')).toBeTruthy();
    expect(screen.queryByText('Start')).toBeNull();
    expect(screen.queryByText('Stop')).toBeNull();
    expect(screen.queryByTestId('sidebar-server-start')).toBeNull();
    expect(screen.queryByTestId('sidebar-server-stop')).toBeNull();
  });
});
