import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerDiagnosis, ServerWithStatus_Serialize } from '$lib/ipc/bindings';
import { markSeen, storageKey } from '$lib/onboarding/contextual-tours';

// Heavy tab bodies + dialog + banner are stubbed so the panel mounts in
// happy-dom without their transitive deps (console buffer, hosting IPC, …).
// The header, tab BAR, hero and wizard branches under test stay real.
vi.mock('$lib/servers/overview/ServerOverviewTab.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/settings/ServerSettingsTab.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/addons/ServerAddonsTab.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerHostingTab.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerBackupsView.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerToInstanceDialog.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerDiagnosisBanner.svelte', () => ({ default: stubComponent() }));

// The wizard stays REAL (the wizard-branch test asserts its heading); this
// wrapper only captures the props the panel passes so the onDone contract can
// be pinned without driving the whole create flow through the wizard UI.
const wizardProps = vi.hoisted(() => ({
  current: null as null | { onDone: (createdId?: string) => void },
}));
vi.mock('$lib/servers/ServerCreateWizard.svelte', async (importOriginal) => {
  const orig = (await importOriginal()) as {
    default: (anchor: unknown, props: unknown) => unknown;
  };
  return {
    default: (anchor: unknown, props: unknown) => {
      wizardProps.current = props as { onDone: (createdId?: string) => void };
      return orig.default(anchor, props);
    },
  };
});

// A minimal no-op Svelte 5 component for child stubs (Svelte 5 mounts a
// component by CALLING it, so a plain function that renders nothing works).
function stubComponent() {
  return function noopComponent() {
    return {};
  };
}

const { serverList, serverStart, serverStop, serverRestart, serverDiagnose, getDataLocation } =
  vi.hoisted(() => ({
    serverList: vi.fn(),
    serverStart: vi.fn(),
    serverStop: vi.fn(),
    serverRestart: vi.fn(),
    serverDiagnose: vi.fn(),
    getDataLocation: vi.fn(),
  }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    serverList,
    serverStart,
    serverStop,
    serverRestart,
    serverDiagnose,
    getDataLocation,
    // ServerCreateWizard children (MemorySlider / core + loader pickers).
    instanceMemoryBounds: vi.fn().mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      default_mb: 2048,
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
import { dataLocation } from '$lib/settings/data-location.svelte';
import { diagnosisDismiss } from '$lib/ui/diagnosis-dismiss.svelte';

function dataLocationStatus(fellBack: boolean) {
  return {
    status: 'ok' as const,
    data: {
      effective: 'C:\\Users\\test\\AppData\\Roaming\\com.lucerna.app',
      configured: fellBack ? 'D:\\LucernaData' : null,
      fell_back: fellBack,
    },
  };
}

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
    wizardProps.current = null;
    serversUi.selectServer(null);
    serversUi.activeTab = 'overview';
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

  it('renders the 5-tab bar for a selected server and flips the shared activeTab', async () => {
    await load([makeServer('a', false)]);
    serversUi.selectServer('a');
    render(ServersPanel, baseProps());
    expect(screen.getAllByRole('tab')).toHaveLength(5);
    await fireEvent.click(screen.getByRole('tab', { name: 'Settings' }));
    expect(serversUi.activeTab).toBe('settings');
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

  it('wizard onDone closes the wizard and auto-selects the created server', async () => {
    await load([]);
    serversUi.creating = true;
    render(ServersPanel, baseProps());
    expect(wizardProps.current).not.toBeNull();

    wizardProps.current?.onDone('new-id');
    expect(serversUi.creating).toBe(false);
    expect(serversUi.selectedServerId).toBe('new-id');
  });

  it('wizard onDone without an id (cancelled import) closes without touching the selection', async () => {
    await load([]);
    serversUi.creating = true;
    render(ServersPanel, baseProps());

    wizardProps.current?.onDone();
    expect(serversUi.creating).toBe(false);
    expect(serversUi.selectedServerId).toBeNull();
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

  // The render gate is unique to this panel: a refused start / stop / restart
  // answers the click inline, unless the banner ON SCREEN says the same thing
  // with its fix attached (a start refused for the EULA). The banner describes
  // the last run or a pre-spawn blocker — never the refusal itself — and a
  // "none" diagnosis draws no banner at all, so neither may hide the error
  // (2026-10-02 regression F06: «Запустить» did nothing visible).
  describe('inline action error vs diagnosis banner', () => {
    function diagnosisOf(
      patternId: string | null,
      overrides: Partial<ServerDiagnosis> = {},
    ): ServerDiagnosis {
      return {
        status: patternId === null ? 'none' : 'actionable',
        diagnosis:
          patternId === null
            ? null
            : {
                pattern_id: patternId,
                title: '',
                explanation: '',
                recommendation: '',
                matched_excerpt: '',
                repair: null,
              },
        client_mods: [],
        forge_skip_count: null,
        log_signature: null,
        server_repair: null,
        port_in_use: null,
        orphan_pid: null,
        corrupt_jar: null,
        suggested_heap_mb: null,
        conflict_mods: [],
        suggested_port: null,
        exit_code: null,
        ...overrides,
      };
    }

    // Selects a stopped server, lets the on-select diagnose() land, then runs a
    // start the backend refuses before spawning.
    async function refusedStart(id: string, diag: ServerDiagnosis, error: { kind: string }) {
      await load([makeServer(id, false)]);
      serverDiagnose.mockResolvedValue({ status: 'ok', data: diag });
      serverStart.mockResolvedValue({ status: 'error', error });
      serversUi.selectServer(id);
      render(ServersPanel, baseProps());
      await vi.waitFor(() => expect(serverState.diagnosisFor(id)).toBeDefined());
      await serverState.start(id);
      await vi.waitFor(() => expect(serverStart).toHaveBeenCalled());
    }

    it('shows a refused start when the diagnosis is "none" (no banner at all)', async () => {
      await refusedStart('refuse-none', diagnosisOf(null), {
        kind: 'server_saved_mc_version_missing',
      });
      expect(screen.getByTestId('server-action-error').textContent).toContain(
        'This server has no Minecraft version recorded',
      );
    });

    it('shows a refused start next to a banner about an earlier crash', async () => {
      await refusedStart(
        'refuse-stale',
        diagnosisOf('server-out-of-memory', { log_signature: 'old-run' }),
        { kind: 'server_saved_mc_version_missing' },
      );
      expect(screen.getByTestId('server-action-error').textContent).toContain(
        'This server has no Minecraft version recorded',
      );
    });

    it('leaves out a refused start the EULA banner on screen already states', async () => {
      await refusedStart(
        'refuse-eula',
        diagnosisOf('server-eula-not-accepted', { server_repair: 'accept_eula' }),
        { kind: 'server_eula_not_accepted' },
      );
      expect(screen.queryByTestId('server-action-error')).toBeNull();
    });

    it('shows the EULA refusal once its banner has been dismissed', async () => {
      diagnosisDismiss.dismiss('server:refuse-eula-dismissed', 'server-eula-not-accepted|');
      await refusedStart(
        'refuse-eula-dismissed',
        diagnosisOf('server-eula-not-accepted', { server_repair: 'accept_eula' }),
        { kind: 'server_eula_not_accepted' },
      );
      expect(screen.getByTestId('server-action-error').textContent).toContain(
        'You must accept the Minecraft EULA',
      );
    });

    it('shows a refused restart of a running server — no banner shows while it runs', async () => {
      await load([makeServer('banner-a', true)]);
      serverDiagnose.mockResolvedValue({
        status: 'ok',
        data: diagnosisOf('server-port-in-use', {
          server_repair: 'change_port',
          port_in_use: 25565,
          suggested_port: 25566,
        }),
      });
      serverRestart.mockResolvedValue({
        status: 'error',
        error: { kind: 'server_already_running' },
      });
      serversUi.selectServer('banner-a');
      render(ServersPanel, baseProps());

      // Wait for the on-select diagnose() (a real store call here) to land
      // before triggering the failing action.
      await vi.waitFor(() => expect(serverState.diagnosisFor('banner-a')).toBeDefined());

      await fireEvent.click(screen.getByText('Restart'));
      await vi.waitFor(() => expect(serverRestart).toHaveBeenCalled());
      expect(screen.getByTestId('server-action-error').textContent).toContain(
        'This server is already running',
      );
    });

    it('shows the inline action error when there is no diagnosis banner (fallback)', async () => {
      await load([makeServer('banner-b', true)]);
      // Default beforeEach mock resolves serverDiagnose with status 'error', so
      // the store never populates a diagnosis for this server.
      serverRestart.mockResolvedValue({
        status: 'error',
        error: { kind: 'server_already_running' },
      });
      serversUi.selectServer('banner-b');
      render(ServersPanel, baseProps());

      await fireEvent.click(screen.getByText('Restart'));
      await vi.waitFor(() => expect(serverRestart).toHaveBeenCalled());
      expect(screen.getByTestId('server-action-error').textContent).toContain(
        'This server is already running',
      );
    });
  });

  describe('data-root fallback gating (§7)', () => {
    // `dataLocation` is a module singleton — restore fell_back=false after each
    // gating test so the gate doesn't leak into the rest of the file.
    afterEach(async () => {
      getDataLocation.mockResolvedValue(dataLocationStatus(false));
      await dataLocation.refresh();
    });

    it('disables the hero create while the data root fell back; click is a no-op', async () => {
      await load([]);
      getDataLocation.mockResolvedValue(dataLocationStatus(true));
      await dataLocation.refresh();

      render(ServersPanel, baseProps());
      const btn = screen.getByTestId('servers-hero-create') as HTMLButtonElement;
      expect(btn.disabled).toBe(true);
      await fireEvent.click(btn);
      expect(serversUi.creating).toBe(false);
    });

    it('leaves the hero create enabled when the data root is healthy', async () => {
      await load([]);
      getDataLocation.mockResolvedValue(dataLocationStatus(false));
      await dataLocation.refresh();

      render(ServersPanel, baseProps());
      const btn = screen.getByTestId('servers-hero-create') as HTMLButtonElement;
      expect(btn.disabled).toBe(false);
    });
  });
});
