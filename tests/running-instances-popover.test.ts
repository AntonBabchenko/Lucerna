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
}) {
  runningInstances.mockResolvedValue(props.rows);
  return render(RunningInstancesPopover, {
    props: {
      runningCount: props.runningCount,
      instanceName: (id: string) => NAMES[id] ?? id,
      onOpenInstance: props.onOpenInstance ?? (() => {}),
    },
  });
}

describe('RunningInstancesPopover', () => {
  beforeEach(() => {
    runningInstances.mockReset();
    stopInstance.mockReset();
    stopInstance.mockResolvedValue({ status: 'ok', data: null });
    // Each event listener resolves to a no-op unlisten fn.
    spawnListen.mockReset().mockResolvedValue(() => {});
    exitListen.mockReset().mockResolvedValue(() => {});
  });

  it('renders the pill with the count and an accessible label', () => {
    renderPopover({ runningCount: 2, rows: [] });
    const pill = screen.getByTestId('running-instances-pill');
    expect(pill.getAttribute('aria-label')).toBe('2 running');
    expect(pill.textContent).toContain('2');
    expect(pill.getAttribute('aria-expanded')).toBe('false');
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
});
