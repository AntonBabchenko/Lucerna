import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';

// The servers popover reads runtime state straight from the `serverState` rune
// module (no backend fetch, no event listeners of its own) and jumps to a
// server via `serversUi`. Mock both so a test controls the running list, the
// per-server busy state, and the lifecycle/jump spies without a Tauri host.
const { state, actionFor, actionErrorFor, stop, restart, setMode, selectServer } = vi.hoisted(
  () => ({
    // Mutable holder read by the `list` getter at access time (the mock factory
    // is hoisted above this, but the getter body runs later, once it exists).
    state: {
      list: [] as Array<{ id: string; name: string; running: boolean; port: number | null }>,
    },
    actionFor: vi.fn(),
    actionErrorFor: vi.fn(),
    stop: vi.fn(),
    restart: vi.fn(),
    setMode: vi.fn(),
    selectServer: vi.fn(),
  }),
);

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get list() {
      return state.list;
    },
    actionFor,
    actionErrorFor,
    stop,
    restart,
  },
}));

vi.mock('$lib/servers/servers-ui.svelte', () => ({
  serversUi: { setMode, selectServer },
}));

import RunningServersPopover from '$lib/layout/RunningServersPopover.svelte';
import { countPillClass } from '$lib/ui/cards/CountPill.svelte';

function makeServer(id: string, name: string, running = true, port: number | null = null) {
  return { id, name, running, port };
}

describe('RunningServersPopover', () => {
  beforeEach(() => {
    // Pin the locale so text assertions ('2 running', 'Running servers', …) hold
    // regardless of the runner's OS locale.
    locale.set('en');
    state.list = [];
    actionFor.mockReset().mockReturnValue(null);
    actionErrorFor.mockReset().mockReturnValue(null);
    stop.mockReset().mockResolvedValue({ ok: true });
    restart.mockReset().mockResolvedValue({ ok: true });
    setMode.mockReset();
    selectServer.mockReset();
  });

  it('renders the pill with the count, accessible label, and collapsed state', () => {
    state.list = [makeServer('a', 'Alpha'), makeServer('b', 'Beta')];
    render(RunningServersPopover, { props: { runningCount: 2 } });
    const pill = screen.getByTestId('running-servers-pill');
    expect(pill.getAttribute('aria-label')).toBe('2 running');
    expect(pill.textContent).toContain('2');
    expect(pill.getAttribute('aria-expanded')).toBe('false');
  });

  it('the compact trigger IS the shared count pill, not a local recipe', () => {
    state.list = [makeServer('a', 'Alpha'), makeServer('b', 'Beta')];
    render(RunningServersPopover, { props: { runningCount: 2, compact: true } });
    const pill = screen.getByTestId('running-servers-pill');
    for (const cls of countPillClass('sm').split(' ')) {
      expect(pill.classList.contains(cls), `missing ${cls}`).toBe(true);
    }
    expect(pill.tagName).toBe('BUTTON');
    expect(pill.getAttribute('aria-expanded')).toBe('false');
  });

  it('stays byte-identical to its RunningInstancesPopover twin', () => {
    // The two popovers are separate components that must not drift; asserting
    // both against ONE builder is what makes that structural rather than a
    // convention. Holds only while the pill is CLOSED — `class:z-50={open}`
    // appends a class once opened — so it is written against a fresh render.
    state.list = [makeServer('a', 'Alpha')];
    render(RunningServersPopover, { props: { runningCount: 1, compact: true } });
    const pill = screen.getByTestId('running-servers-pill');
    expect(pill.className.trim()).toBe(countPillClass('sm'));
  });

  it('opens on click and lists the running servers by name (aria-expanded toggles)', async () => {
    state.list = [makeServer('a', 'Alpha'), makeServer('b', 'Beta')];
    render(RunningServersPopover, { props: { runningCount: 2 } });
    const pill = screen.getByTestId('running-servers-pill');
    await fireEvent.click(pill);
    expect(pill.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByTestId('running-servers-popover')).toBeTruthy();
    expect(screen.getByText('Alpha')).toBeTruthy();
    expect(screen.getByText('Beta')).toBeTruthy();
  });

  it('Stop calls serverState.stop for that server', async () => {
    state.list = [makeServer('a', 'Alpha')];
    render(RunningServersPopover, { props: { runningCount: 1 } });
    await fireEvent.click(screen.getByTestId('running-servers-pill'));
    await fireEvent.click(screen.getByTestId('running-servers-stop-a'));
    expect(stop).toHaveBeenCalledWith('a');
    expect(restart).not.toHaveBeenCalled();
  });

  it('Restart calls serverState.restart for that server', async () => {
    state.list = [makeServer('a', 'Alpha')];
    render(RunningServersPopover, { props: { runningCount: 1 } });
    await fireEvent.click(screen.getByTestId('running-servers-pill'));
    await fireEvent.click(screen.getByTestId('running-servers-restart-a'));
    expect(restart).toHaveBeenCalledWith('a');
    expect(stop).not.toHaveBeenCalled();
  });

  it('Open jumps to the server in servers mode and closes the popover', async () => {
    state.list = [makeServer('a', 'Alpha')];
    render(RunningServersPopover, { props: { runningCount: 1 } });
    await fireEvent.click(screen.getByTestId('running-servers-pill'));
    await fireEvent.click(screen.getByTestId('running-servers-open-a'));
    expect(setMode).toHaveBeenCalledWith('servers');
    expect(selectServer).toHaveBeenCalledWith('a');
    await waitFor(() => expect(screen.queryByTestId('running-servers-popover')).toBeNull());
  });

  // Regression: a server with an in-flight action (actionFor !== null) must have
  // its Stop/Restart disabled, so a rapid double-click can't re-enter act() —
  // which would surface a spurious "Stop: null" toast off runLifecycle's silent
  // concurrency guard. Open stays unaffected (jumping is always safe).
  it('disables Stop/Restart for a server with an in-flight action', async () => {
    state.list = [makeServer('a', 'Alpha')];
    actionFor.mockImplementation((id: string) => (id === 'a' ? 'stop' : null));
    render(RunningServersPopover, { props: { runningCount: 1 } });
    await fireEvent.click(screen.getByTestId('running-servers-pill'));
    expect((screen.getByTestId('running-servers-stop-a') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId('running-servers-restart-a') as HTMLButtonElement).disabled).toBe(
      true,
    );
    expect((screen.getByTestId('running-servers-open-a') as HTMLButtonElement).disabled).toBe(
      false,
    );
  });
});
