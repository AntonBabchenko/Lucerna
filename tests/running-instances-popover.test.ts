import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { RunningInstanceInfo } from '$lib/ipc/bindings';

// Backend seam: the popover fetches the authoritative running list on open and
// stops an instance via commands; it refreshes on spawn/exit events. Mock all
// four so the component can mount and drive without a real Tauri host.
const { runningInstances, stopInstance, spawnListen, exitListen } = vi.hoisted(() => ({
  runningInstances: vi.fn(),
  stopInstance: vi.fn(),
  spawnListen: vi.fn(),
  exitListen: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { runningInstances, stopInstance },
  events: {
    processSpawned: { listen: spawnListen },
    processExited: { listen: exitListen },
  },
}));

import RunningInstancesPopover from '$lib/layout/RunningInstancesPopover.svelte';
import { countPillClass } from '$lib/ui/cards/CountPill.svelte';

// Captured event callbacks the component registers on open, so a test can fire
// a spawn/exit and drive the resulting refresh (the component's listener bodies
// ignore the payload and just re-fetch, but we pass a realistic one anyway).
type EventCb = (event: { payload: { instance_id: string } }) => void;
let capturedSpawn: EventCb | null = null;
let capturedExit: EventCb | null = null;

const NAMES: Record<string, string> = { a: 'Alpha', b: 'Beta' };

function makeRow(id: string, startedMsAgo: number | null): RunningInstanceInfo {
  return {
    instance_id: id,
    pid: 1234,
    max_heap_mb: 2048,
    started_unix_ms: startedMsAgo === null ? null : Date.now() - startedMsAgo,
  };
}

function renderPopover(props: {
  runningCount: number;
  rows: RunningInstanceInfo[];
  onOpenInstance?: (id: string) => void;
  /** The narrow ModeSwitcher-rail trigger; the default is the wide one. */
  compact?: boolean;
}) {
  runningInstances.mockResolvedValue(props.rows);
  return render(RunningInstancesPopover, {
    props: {
      runningCount: props.runningCount,
      instanceName: (id: string) => NAMES[id] ?? id,
      onOpenInstance: props.onOpenInstance ?? (() => {}),
      compact: props.compact ?? false,
    },
  });
}

describe('RunningInstancesPopover', () => {
  beforeEach(() => {
    runningInstances.mockReset();
    stopInstance.mockReset();
    stopInstance.mockResolvedValue({ status: 'ok', data: null });
    // Each event listener captures the callback it's given (so a test can fire
    // it) and resolves to a no-op unlisten fn.
    capturedSpawn = null;
    capturedExit = null;
    spawnListen.mockReset().mockImplementation((cb: EventCb) => {
      capturedSpawn = cb;
      return Promise.resolve(() => {});
    });
    exitListen.mockReset().mockImplementation((cb: EventCb) => {
      capturedExit = cb;
      return Promise.resolve(() => {});
    });
  });

  it('renders the pill with the count and an accessible label', () => {
    renderPopover({ runningCount: 2, rows: [] });
    const pill = screen.getByTestId('running-instances-pill');
    expect(pill.getAttribute('aria-label')).toBe('2 running');
    expect(pill.textContent).toContain('2');
    expect(pill.getAttribute('aria-expanded')).toBe('false');
  });

  it('the compact trigger IS the shared count pill, not a local recipe', () => {
    renderPopover({ runningCount: 2, rows: [], compact: true });
    const pill = screen.getByTestId('running-instances-pill');
    for (const cls of countPillClass('sm').split(' ')) {
      expect(pill.classList.contains(cls), `missing ${cls}`).toBe(true);
    }
    // The trigger stays the focusable control: the primitive supplies looks
    // only, never the button semantics.
    expect(pill.tagName).toBe('BUTTON');
    expect(pill.getAttribute('aria-label')).toBe('2 running');
    expect(pill.getAttribute('aria-expanded')).toBe('false');
  });

  it('the expanded (non-compact) trigger keeps its own wider shape', () => {
    // Guard against over-reach: only the COMPACT branch is a count pill. The
    // h-7 bg-black/20 trigger is a different control and must not be collapsed
    // into the primitive.
    renderPopover({ runningCount: 2, rows: [] });
    const pill = screen.getByTestId('running-instances-pill');
    expect(pill.className).toContain('h-7');
    expect(pill.className).not.toContain('h-[15px]');
  });

  it('opens the popover on click and lists running instances with elapsed time', async () => {
    renderPopover({
      runningCount: 2,
      rows: [makeRow('a', 5 * 60 * 1000), makeRow('b', null)],
    });
    await fireEvent.click(screen.getByTestId('running-instances-pill'));
    // List is fetched from the backend on open.
    await waitFor(() => expect(runningInstances).toHaveBeenCalled());
    expect(await screen.findByText('Alpha')).toBeTruthy();
    expect(screen.getByText('Beta')).toBeTruthy();
    // Alpha started 5m ago → short duration label; Beta has no start → dash.
    expect(screen.getByText('5m')).toBeTruthy();
    expect(screen.getByText('—')).toBeTruthy();
  });

  it('Open jumps to the instance and closes the popover', async () => {
    const onOpenInstance = vi.fn();
    renderPopover({ runningCount: 1, rows: [makeRow('a', 60_000)], onOpenInstance });
    await fireEvent.click(screen.getByTestId('running-instances-pill'));
    const openBtn = await screen.findByTestId('running-instances-open-a');
    await fireEvent.click(openBtn);
    expect(onOpenInstance).toHaveBeenCalledWith('a');
    await waitFor(() => expect(screen.queryByTestId('running-instances-popover')).toBeNull());
  });

  it('Stop calls stop_instance for that instance', async () => {
    renderPopover({ runningCount: 1, rows: [makeRow('a', 60_000)] });
    await fireEvent.click(screen.getByTestId('running-instances-pill'));
    const stopBtn = await screen.findByTestId('running-instances-stop-a');
    await fireEvent.click(stopBtn);
    expect(stopInstance).toHaveBeenCalledWith('a');
  });

  it('closes when the fetched list is empty (all exited between open and fetch)', async () => {
    renderPopover({ runningCount: 1, rows: [] });
    await fireEvent.click(screen.getByTestId('running-instances-pill'));
    await waitFor(() => expect(runningInstances).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByTestId('running-instances-popover')).toBeNull());
  });

  // Regression: after Stop A → A exits → A relaunches, A's Stop button must be
  // re-armed. The stopping-set is pruned on each refresh (a row that's gone
  // drops its stale entry); without that prune the relaunched row would keep a
  // permanently-disabled Stop button until the count hit 0 and the component
  // unmounted — breaking exactly the multi-instance flow this feature exists for.
  it('re-arms Stop for a relaunched instance (stop → exit → relaunch)', async () => {
    const rowA = makeRow('a', 60_000);
    const rowB = makeRow('b', 60_000);
    runningInstances.mockResolvedValue([rowA, rowB]);
    render(RunningInstancesPopover, {
      props: {
        runningCount: 2,
        instanceName: (id: string) => NAMES[id] ?? id,
        onOpenInstance: () => {},
      },
    });

    // Open → both rows present.
    await fireEvent.click(screen.getByTestId('running-instances-pill'));
    const stopA = await screen.findByTestId('running-instances-stop-a');
    expect(screen.getByTestId('running-instances-stop-b')).toBeTruthy();

    // Stop A → command fired; A's button disarms optimistically until refresh.
    await fireEvent.click(stopA);
    expect(stopInstance).toHaveBeenCalledWith('a');
    await waitFor(() =>
      expect((screen.getByTestId('running-instances-stop-a') as HTMLButtonElement).disabled).toBe(
        true,
      ),
    );

    // A exits: backend now reports only B; fire the exit listener → refresh
    // drops A's row (and prunes 'a' from the stopping-set).
    runningInstances.mockResolvedValue([rowB]);
    expect(capturedExit).toBeTypeOf('function');
    capturedExit?.({ payload: { instance_id: 'a' } });
    await waitFor(() => expect(screen.queryByTestId('running-instances-stop-a')).toBeNull());

    // A relaunches: backend reports A + B again; fire the spawn listener →
    // refresh brings A's row back.
    runningInstances.mockResolvedValue([rowA, rowB]);
    expect(capturedSpawn).toBeTypeOf('function');
    capturedSpawn?.({ payload: { instance_id: 'a' } });

    // A's Stop button is back AND NOT disabled — the stale stopping entry was
    // pruned, so the relaunched instance is stoppable again.
    const stopAAgain = await screen.findByTestId('running-instances-stop-a');
    expect((stopAAgain as HTMLButtonElement).disabled).toBe(false);
  });
});
