import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerDiagnosis } from '$lib/ipc/bindings';
import ServerDiagnosisBanner from '$lib/servers/ServerDiagnosisBanner.svelte';

// Mock bindings — not needed for this component directly but imported transitively.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {},
  events: {},
}));

// Mock toasts — pushSuccess is called on successful removal (banner unmounts after, so toast
// is the only visible confirmation). The mock lets the import resolve without the Svelte
// runes runtime.
const pushSuccessMock = vi.fn();
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: (...args: unknown[]) => pushSuccessMock(...args),
}));

// Shared mutable diagnosis store keyed by serverId.
// NOTE: object literal must be plain — no references to outer variables —
// because vi.mock factories are hoisted before any top-level declarations.
const mockDiagnoses: Record<string, ServerDiagnosis | undefined> = {};

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get list() {
      return [];
    },
    diagnosisFor: (id: string) => mockDiagnoses[id],
    diagnose: vi.fn().mockResolvedValue(undefined),
    removeClientMods: vi.fn().mockResolvedValue({ ok: true }),
    acceptEula: vi.fn().mockResolvedValue({ ok: true }),
    stopOrphan: vi.fn().mockResolvedValue({ ok: true }),
    changePort: vi.fn().mockResolvedValue({ ok: true }),
    running: (_id: string) => false,
    refresh: vi.fn().mockResolvedValue(undefined),
    init: vi.fn(),
  },
}));

// removeClientMods spy — extracted in beforeAll from the mocked module.
let removeClientModsSpy: ReturnType<typeof vi.fn>;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeClientOnlyDiagnosis(overrides: Partial<ServerDiagnosis> = {}): ServerDiagnosis {
  return {
    status: 'actionable',
    diagnosis: {
      pattern_id: 'server-client-only-mod-crash',
      title: 'A client-only mod crashed the server',
      explanation: 'A mod meant for the game client was loaded on the dedicated server.',
      recommendation: 'Remove the client-only mods below, then start the server again.',
      matched_excerpt: '',
      repair: null,
    },
    client_mods: [],
    forge_skip_count: null,
    log_signature: 'sig-abc',
    server_repair: null,
    port_in_use: null,
    orphan_pid: null,
    ...overrides,
  };
}

// Pre-spawn (class A) diagnosis: a fixable launch outcome with a server_repair tag.
function makePreflightDiagnosis(
  patternId: string,
  serverRepair: NonNullable<ServerDiagnosis['server_repair']>,
  overrides: Partial<ServerDiagnosis> = {},
): ServerDiagnosis {
  return {
    status: 'actionable',
    diagnosis: {
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
    server_repair: serverRepair,
    port_in_use: null,
    orphan_pid: null,
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('ServerDiagnosisBanner', () => {
  beforeAll(async () => {
    locale.set('en');
    const mod = await import('$lib/servers/server-state.svelte');
    // mod.serverState.removeClientMods is the vi.fn() from our factory above.
    removeClientModsSpy = mod.serverState.removeClientMods as ReturnType<typeof vi.fn>;
  });

  it('renders nothing when diagnosisFor returns undefined', () => {
    mockDiagnoses['srv-none'] = undefined;
    const { container } = render(ServerDiagnosisBanner, {
      props: { serverId: 'srv-none' },
    });
    expect(container.querySelector('[data-testid="server-diagnosis-banner"]')).toBeNull();
  });

  it('renders nothing when status is handled', () => {
    mockDiagnoses['srv-handled'] = makeClientOnlyDiagnosis({
      status: 'handled',
    });
    const { container } = render(ServerDiagnosisBanner, {
      props: { serverId: 'srv-handled' },
    });
    expect(container.querySelector('[data-testid="server-diagnosis-banner"]')).toBeNull();
  });

  it('renders nothing when status is none', () => {
    mockDiagnoses['srv-none-status'] = makeClientOnlyDiagnosis({
      status: 'none',
    });
    const { container } = render(ServerDiagnosisBanner, {
      props: { serverId: 'srv-none-status' },
    });
    expect(container.querySelector('[data-testid="server-diagnosis-banner"]')).toBeNull();
  });

  it('shows the client-only title when status is actionable', () => {
    mockDiagnoses['srv-actionable'] = makeClientOnlyDiagnosis();
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-actionable' } });

    // i18n key: servers.diagnose.clientOnly.title
    expect(screen.getByText('A client-only mod crashed the server')).toBeTruthy();
  });

  it('shows advisory diagnosis without actionable checklist', () => {
    mockDiagnoses['srv-advisory'] = makeClientOnlyDiagnosis({
      status: 'advisory',
      client_mods: [{ filename: 'optifine.jar', reason: 'crash', confidence: 'high' }],
    });
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-advisory' } });

    // Banner visible
    expect(screen.getByTestId('server-diagnosis-banner')).toBeTruthy();
    // Show client mods toggle should NOT appear for advisory (only for actionable)
    expect(screen.queryByText('Show client-only mods')).toBeNull();
  });

  it('pre-checks high-confidence mods and does NOT pre-check medium-confidence', async () => {
    mockDiagnoses['srv-checklist'] = makeClientOnlyDiagnosis({
      client_mods: [
        { filename: 'high-mod.jar', reason: 'manifest_client', confidence: 'high' },
        { filename: 'medium-mod.jar', reason: 'crash', confidence: 'medium' },
      ],
    });

    render(ServerDiagnosisBanner, { props: { serverId: 'srv-checklist' } });

    // Click the toggle to reveal the checklist
    const toggle = screen.getByText('Show client-only mods');
    await fireEvent.click(toggle);

    // Locate checkboxes by id (id="mod-<filename>")
    const highCb = document.getElementById('mod-high-mod.jar') as HTMLInputElement;
    const medCb = document.getElementById('mod-medium-mod.jar') as HTMLInputElement;

    expect(highCb).not.toBeNull();
    expect(medCb).not.toBeNull();
    expect(highCb.checked).toBe(true);
    expect(medCb.checked).toBe(false);
  });

  it('calls removeClientMods with only the checked filenames and toasts on success', async () => {
    removeClientModsSpy.mockResolvedValue({ ok: true });
    pushSuccessMock.mockClear();

    mockDiagnoses['srv-remove'] = makeClientOnlyDiagnosis({
      client_mods: [
        { filename: 'alpha.jar', reason: 'manifest_client', confidence: 'high' },
        { filename: 'beta.jar', reason: 'crash', confidence: 'medium' },
      ],
    });

    render(ServerDiagnosisBanner, { props: { serverId: 'srv-remove' } });

    // Open checklist
    await fireEvent.click(screen.getByText('Show client-only mods'));

    // Uncheck alpha (was pre-checked high)
    const alphaCb = document.getElementById('mod-alpha.jar') as HTMLInputElement;
    await fireEvent.change(alphaCb, { target: { checked: false } });

    // Check beta manually
    const betaCb = document.getElementById('mod-beta.jar') as HTMLInputElement;
    await fireEvent.change(betaCb, { target: { checked: true } });

    // Click Remove selected
    await fireEvent.click(screen.getByText('Remove selected'));

    expect(removeClientModsSpy).toHaveBeenCalledWith('srv-remove', ['beta.jar'], 'sig-abc');
    expect(pushSuccessMock).toHaveBeenCalledOnce();
  });

  it('shows the Accept EULA fix button for the accept_eula repair', () => {
    mockDiagnoses['srv-eula'] = makePreflightDiagnosis('server-eula-not-accepted', 'accept_eula');
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-eula' } });
    expect(screen.getByTestId('server-fix-accept-eula')).toBeTruthy();
  });

  it('shows the Stop-orphan fix button carrying the pid', async () => {
    const mod = await import('$lib/servers/server-state.svelte');
    const stopOrphanSpy = mod.serverState.stopOrphan as ReturnType<typeof vi.fn>;
    stopOrphanSpy.mockClear();
    mockDiagnoses['srv-orphan'] = makePreflightDiagnosis(
      'server-orphan-running',
      'stop_orphan_and_retry',
      { orphan_pid: 9999 },
    );
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-orphan' } });
    const btn = screen.getByTestId('server-fix-stop-orphan');
    expect(btn).toBeTruthy();
    await fireEvent.click(btn);
    expect(stopOrphanSpy).toHaveBeenCalledWith('srv-orphan', 9999);
  });

  it('shows the Change-port fix button suggesting the next port', async () => {
    const mod = await import('$lib/servers/server-state.svelte');
    const changePortSpy = mod.serverState.changePort as ReturnType<typeof vi.fn>;
    changePortSpy.mockClear();
    mockDiagnoses['srv-port'] = makePreflightDiagnosis('server-port-in-use', 'change_port', {
      port_in_use: 25565,
    });
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-port' } });
    const btn = screen.getByTestId('server-fix-change-port');
    expect(btn).toBeTruthy();
    await fireEvent.click(btn);
    // Suggests port_in_use + 1.
    expect(changePortSpy).toHaveBeenCalledWith('srv-port', 25566);
  });
});
