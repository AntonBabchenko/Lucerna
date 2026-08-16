import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerDiagnosis } from '$lib/ipc/bindings';
import ServerDiagnosisBanner from '$lib/servers/ServerDiagnosisBanner.svelte';
import { diagnosisDismiss } from '$lib/ui/diagnosis-dismiss.svelte';

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
// Per-server running flag (unset → not running).
const mockRunning: Record<string, boolean> = {};

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
    raiseHeap: vi.fn().mockResolvedValue({ ok: true }),
    lowerHeap: vi.fn().mockResolvedValue({ ok: true }),
    redownloadJar: vi.fn().mockResolvedValue({ ok: true }),
    disableMods: vi.fn().mockResolvedValue({ ok: true }),
    installMissingDep: vi.fn().mockResolvedValue({ ok: true }),
    running: (id: string) => mockRunning[id] ?? false,
    refresh: vi.fn().mockResolvedValue(undefined),
    init: vi.fn(),
  },
}));

// removeClientMods spy — extracted in beforeAll from the mocked module.
let removeClientModsSpy: ReturnType<typeof vi.fn>;
// acceptEula spy — same pattern; the failure-rendering tests drive it directly.
let acceptEulaSpy: ReturnType<typeof vi.fn>;

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
    corrupt_jar: null,
    suggested_heap_mb: null,
    conflict_mods: [],
    suggested_port: null,
    exit_code: null,
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
    corrupt_jar: null,
    suggested_heap_mb: null,
    conflict_mods: [],
    suggested_port: null,
    exit_code: null,
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
    acceptEulaSpy = mod.serverState.acceptEula as ReturnType<typeof vi.fn>;
  });

  beforeEach(() => {
    // The dismiss store is a module singleton — clear it so dismissals don't leak.
    diagnosisDismiss.reset();
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

  it('renders nothing while the server is running, even with an actionable diagnosis', () => {
    // A running server hasn't crashed — a lingering/non-fatal-warning diagnosis
    // must not surface as a crash banner.
    mockDiagnoses['srv-running'] = makeClientOnlyDiagnosis();
    mockRunning['srv-running'] = true;
    const { container } = render(ServerDiagnosisBanner, {
      props: { serverId: 'srv-running' },
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

  it('offers a link to the EULA beside the one-click accept fix', () => {
    // The fix button is itself an act of acceptance, so the document has to be
    // readable from here — not only from the create wizard.
    mockDiagnoses['srv-eula-link'] = makePreflightDiagnosis(
      'server-eula-not-accepted',
      'accept_eula',
    );
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-eula-link' } });
    expect(screen.getByTestId('eula-link').textContent).toContain('Read the Minecraft EULA');
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

  it('shows the Change-port fix button using the backend-probed free port', async () => {
    const mod = await import('$lib/servers/server-state.svelte');
    const changePortSpy = mod.serverState.changePort as ReturnType<typeof vi.fn>;
    changePortSpy.mockClear();
    // Busy port is 25566 but the next free one is 25570 — the button must use the
    // backend's suggested_port, NOT a blind current+1 (which could be busy/no-op).
    mockDiagnoses['srv-port'] = makePreflightDiagnosis('server-port-in-use', 'change_port', {
      port_in_use: 25566,
      suggested_port: 25570,
    });
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-port' } });
    const btn = screen.getByTestId('server-fix-change-port');
    expect(btn).toBeTruthy();
    await fireEvent.click(btn);
    expect(changePortSpy).toHaveBeenCalledWith('srv-port', 25570);
  });

  it('falls back to current+1 when no suggested_port was probed', async () => {
    const mod = await import('$lib/servers/server-state.svelte');
    const changePortSpy = mod.serverState.changePort as ReturnType<typeof vi.fn>;
    changePortSpy.mockClear();
    mockDiagnoses['srv-port-fallback'] = makePreflightDiagnosis(
      'server-port-in-use',
      'change_port',
      { port_in_use: 25565, suggested_port: null },
    );
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-port-fallback' } });
    await fireEvent.click(screen.getByTestId('server-fix-change-port'));
    expect(changePortSpy).toHaveBeenCalledWith('srv-port-fallback', 25566);
  });

  it('shows the crash-unknown advisory with the exit code (no fix button)', () => {
    // A crash with no recognized cause (e.g. a Windows process-init failure that
    // produced no output): advisory banner naming the exit code, no one-click fix.
    mockDiagnoses['srv-crash'] = makeClientOnlyDiagnosis({
      status: 'advisory',
      diagnosis: {
        pattern_id: 'server-crash-unknown',
        title: 'The server stopped unexpectedly',
        explanation: '',
        recommendation: '',
        matched_excerpt: '',
        repair: null,
      },
      server_repair: null,
      exit_code: -1073741502, // 0xC0000142
    });
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-crash' } });
    expect(screen.getByText('The server stopped unexpectedly')).toBeTruthy();
    // The hex-formatted NTSTATUS code is shown in the explanation.
    expect(screen.getByText(/0xC0000142/)).toBeTruthy();
    // No port/heap/etc fix button for an environmental crash.
    expect(screen.queryByTestId('server-fix-change-port')).toBeNull();
  });

  // --- Phase 2: class-B fix buttons ----------------------------------------

  it('shows Raise-heap button carrying the suggested heap', async () => {
    const mod = await import('$lib/servers/server-state.svelte');
    const raiseHeapSpy = mod.serverState.raiseHeap as ReturnType<typeof vi.fn>;
    raiseHeapSpy.mockClear();
    mockDiagnoses['srv-oom'] = makePreflightDiagnosis('server-out-of-memory', 'raise_heap', {
      suggested_heap_mb: 6144,
    });
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-oom' } });
    const btn = screen.getByTestId('server-fix-raise-heap');
    expect(btn).toBeTruthy();
    await fireEvent.click(btn);
    expect(raiseHeapSpy).toHaveBeenCalledWith('srv-oom', 6144);
  });

  it('shows Lower-heap button carrying the safe max', async () => {
    const mod = await import('$lib/servers/server-state.svelte');
    const lowerHeapSpy = mod.serverState.lowerHeap as ReturnType<typeof vi.fn>;
    lowerHeapSpy.mockClear();
    mockDiagnoses['srv-heap'] = makePreflightDiagnosis('server-heap-too-big', 'lower_heap', {
      suggested_heap_mb: 4096,
    });
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-heap' } });
    const btn = screen.getByTestId('server-fix-lower-heap');
    expect(btn).toBeTruthy();
    await fireEvent.click(btn);
    expect(lowerHeapSpy).toHaveBeenCalledWith('srv-heap', 4096);
  });

  it('shows Redownload-jar button for redownload_server_jar repair', async () => {
    const mod = await import('$lib/servers/server-state.svelte');
    const redownloadSpy = mod.serverState.redownloadJar as ReturnType<typeof vi.fn>;
    redownloadSpy.mockClear();
    mockDiagnoses['srv-corrupt'] = makePreflightDiagnosis(
      'server-corrupt-jar',
      'redownload_server_jar',
      { corrupt_jar: 'sodium.jar' },
    );
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-corrupt' } });
    const btn = screen.getByTestId('server-fix-redownload-jar');
    expect(btn).toBeTruthy();
    await fireEvent.click(btn);
    expect(redownloadSpy).toHaveBeenCalledWith('srv-corrupt');
  });

  it('shows Install-dep button for install_missing_dep repair', async () => {
    const mod = await import('$lib/servers/server-state.svelte');
    const installSpy = mod.serverState.installMissingDep as ReturnType<typeof vi.fn>;
    installSpy.mockClear();
    mockDiagnoses['srv-dep'] = makePreflightDiagnosis('server-missing-dep', 'install_missing_dep', {
      conflict_mods: ['jei'],
    });
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-dep' } });
    const btn = screen.getByTestId('server-fix-install-dep');
    expect(btn).toBeTruthy();
    await fireEvent.click(btn);
    expect(installSpy).toHaveBeenCalledWith('srv-dep', ['jei']);
  });

  it('every fix button carries an aria-label and aria-busy', () => {
    mockDiagnoses['srv-a11y'] = makePreflightDiagnosis('server-port-in-use', 'change_port', {
      port_in_use: 25565,
    });
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-a11y' } });
    const btn = screen.getByTestId('server-fix-change-port');
    expect(btn.getAttribute('aria-label')).toBeTruthy();
    expect(btn.hasAttribute('aria-busy')).toBe(true);
  });

  it('lists named conflict mods as guidance when no installed jar matched', () => {
    mockDiagnoses['srv-conflict'] = makePreflightDiagnosis('server-mod-conflict', 'disable_mods', {
      conflict_mods: ['sodium', 'oldlib'],
    });
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-conflict' } });
    expect(screen.getByTestId('server-fix-disable-mods')).toBeTruthy();
    expect(screen.getByText('sodium')).toBeTruthy();
    expect(screen.getByText('oldlib')).toBeTruthy();
  });

  it('hides the banner when the dismiss button is clicked', async () => {
    mockDiagnoses['srv-dismiss'] = makeClientOnlyDiagnosis();
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-dismiss' } });
    expect(screen.getByTestId('server-diagnosis-banner')).toBeTruthy();
    await fireEvent.click(screen.getByTestId('server-diagnosis-dismiss'));
    expect(screen.queryByTestId('server-diagnosis-banner')).toBeNull();
  });

  it('stays hidden for the same diagnosis but resurfaces for a different one', async () => {
    // Dismiss the client-only crash (signature = pattern|log_signature).
    mockDiagnoses['srv-sig'] = makeClientOnlyDiagnosis();
    const first = render(ServerDiagnosisBanner, { props: { serverId: 'srv-sig' } });
    await fireEvent.click(screen.getByTestId('server-diagnosis-dismiss'));
    expect(screen.queryByTestId('server-diagnosis-banner')).toBeNull();
    first.unmount();

    // Same diagnosis → still hidden on a fresh mount.
    const second = render(ServerDiagnosisBanner, { props: { serverId: 'srv-sig' } });
    expect(screen.queryByTestId('server-diagnosis-banner')).toBeNull();
    second.unmount();

    // A different problem (different pattern) → banner returns.
    mockDiagnoses['srv-sig'] = makePreflightDiagnosis('server-port-in-use', 'change_port', {
      port_in_use: 25565,
    });
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-sig' } });
    expect(screen.getByTestId('server-diagnosis-banner')).toBeTruthy();
  });

  it('re-shows a dismissed banner once the dismissal is cleared (restore)', async () => {
    mockDiagnoses['srv-restore'] = makeClientOnlyDiagnosis();
    const first = render(ServerDiagnosisBanner, { props: { serverId: 'srv-restore' } });
    await fireEvent.click(screen.getByTestId('server-diagnosis-dismiss'));
    expect(screen.queryByTestId('server-diagnosis-banner')).toBeNull();
    first.unmount();
    // The restore badge lives in ServersPanel; here we assert the banner
    // returns once the dismissal is cleared (what that badge does on click).
    diagnosisDismiss.restore('server:srv-restore');
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-restore' } });
    expect(screen.getByTestId('server-diagnosis-banner')).toBeTruthy();
  });

  it('routes disable_mods through disableMods (reversible) for the checklist', async () => {
    const mod = await import('$lib/servers/server-state.svelte');
    const disableSpy = mod.serverState.disableMods as ReturnType<typeof vi.fn>;
    disableSpy.mockClear();
    pushSuccessMock.mockClear();
    mockDiagnoses['srv-mixin'] = makePreflightDiagnosis('server-mixin-crash', 'disable_mods', {
      log_signature: 'sig-mixin',
      client_mods: [{ filename: 'etf.jar', reason: 'crash', confidence: 'high' }],
    });
    render(ServerDiagnosisBanner, { props: { serverId: 'srv-mixin' } });
    await fireEvent.click(screen.getByText('Show client-only mods'));
    // etf.jar is pre-checked (high confidence). Click "Disable selected".
    await fireEvent.click(screen.getByText('Disable selected'));
    expect(disableSpy).toHaveBeenCalledWith('srv-mixin', ['etf.jar'], 'sig-mixin');
  });

  it('shows a thrown transport failure’s message, not "{}"', async () => {
    // The store's one-click fix wrappers have no try/catch, so a transport
    // failure propagates out of acceptEula() and lands in runFix's catch.
    acceptEulaSpy.mockRejectedValue(new Error('ipc channel closed'));
    mockDiagnoses['srv-throw'] = makePreflightDiagnosis(
      'server-eula-not-accepted',
      'accept_eula',
    );

    render(ServerDiagnosisBanner, { props: { serverId: 'srv-throw' } });
    await fireEvent.click(screen.getByTestId('server-fix-accept-eula'));

    const err = await screen.findByTestId('server-fix-error');
    expect(err.textContent?.trim()).toBe('ipc channel closed');
    // The regression this pins: formatError's default arm JSON.stringifies, and
    // a JS Error has no enumerable own properties, so the user saw empty braces.
    expect(err.textContent).not.toContain('{}');
  });

  it('shows the typed reason when the fix command returns an error Result', async () => {
    acceptEulaSpy.mockResolvedValue({
      ok: false,
      error: { kind: 'server_already_running', id: 'srv-typed' },
    });
    mockDiagnoses['srv-typed'] = makePreflightDiagnosis(
      'server-eula-not-accepted',
      'accept_eula',
    );

    render(ServerDiagnosisBanner, { props: { serverId: 'srv-typed' } });
    await fireEvent.click(screen.getByTestId('server-fix-accept-eula'));

    const err = await screen.findByTestId('server-fix-error');
    expect(err.textContent?.trim()).toBe('This server is already running');
  });
});
