import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import ServerConsole from '$lib/servers/ServerConsole.svelte';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverSendCommand: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    serverReadLog: vi.fn().mockResolvedValue({ status: 'ok', data: '' }),
  },
}));

const mockLines: Record<string, string[]> = {};
const mockRunning: Record<string, boolean> = {};

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get list() {
      return [];
    },
    lines: (id: string) => mockLines[id] ?? [],
    running: (id: string) => mockRunning[id] ?? false,
    listLogs: vi.fn().mockResolvedValue({ ok: true, list: [] }),
    openLogsFolder: vi.fn().mockResolvedValue(undefined),
  },
}));

describe('ServerConsole filter + search (#14)', () => {
  beforeAll(() => locale.set('en'));

  const LEVEL_LINES = [
    '[12:00:00] [Server thread/INFO]: Server started',
    '[12:00:01] [Server thread/WARN]: Can not keep up',
    '[12:00:02] [Server thread/ERROR]: Failed to load mod',
  ];

  it('hides a level when its chip is toggled off', async () => {
    mockLines['flt-1'] = LEVEL_LINES;
    mockRunning['flt-1'] = true;

    render(ServerConsole, { props: { serverId: 'flt-1' } });

    // All three lines visible initially.
    expect(screen.getByText('[12:00:00] [Server thread/INFO]: Server started')).toBeTruthy();
    expect(screen.getByText('[12:00:02] [Server thread/ERROR]: Failed to load mod')).toBeTruthy();

    // Toggle the INFO chip off → the INFO line disappears, others remain.
    await fireEvent.click(screen.getByTestId('server-console-level-info'));

    expect(screen.queryByText('[12:00:00] [Server thread/INFO]: Server started')).toBeNull();
    expect(screen.getByText('[12:00:02] [Server thread/ERROR]: Failed to load mod')).toBeTruthy();
  });

  it('counts and navigates search matches', async () => {
    mockLines['flt-2'] = [
      '[12:00:00] [Server thread/INFO]: alpha here',
      '[12:00:01] [Server thread/INFO]: beta then alpha',
    ];
    mockRunning['flt-2'] = true;

    render(ServerConsole, { props: { serverId: 'flt-2' } });

    const search = screen.getByTestId('server-console-search') as HTMLInputElement;
    await fireEvent.input(search, { target: { value: 'alpha' } });

    // Debounced — wait for the match counter to reflect 2 matches.
    await waitFor(() => expect(screen.getByText('1/2')).toBeTruthy());

    // Next match advances the counter.
    await fireEvent.click(screen.getByLabelText('Next match'));
    await waitFor(() => expect(screen.getByText('2/2')).toBeTruthy());
  });

  it('clamps the active match when a level filter shrinks the result set (#14)', async () => {
    mockLines['flt-4'] = [
      '[12:00:00] [Server thread/INFO]: target one',
      '[12:00:01] [Server thread/INFO]: target two',
      '[12:00:02] [Server thread/ERROR]: target three',
    ];
    mockRunning['flt-4'] = true;

    render(ServerConsole, { props: { serverId: 'flt-4' } });

    const search = screen.getByTestId('server-console-search') as HTMLInputElement;
    await fireEvent.input(search, { target: { value: 'target' } });
    await waitFor(() => expect(screen.getByText('1/3')).toBeTruthy());

    // Navigate to the last (3rd) match.
    await fireEvent.click(screen.getByLabelText('Next match'));
    await fireEvent.click(screen.getByLabelText('Next match'));
    await waitFor(() => expect(screen.getByText('3/3')).toBeTruthy());

    // Hide INFO → only the ERROR match remains. The counter must clamp to a
    // real match ("1/1"), never the impossible "3/1".
    await fireEvent.click(screen.getByTestId('server-console-level-info'));
    await waitFor(() => expect(screen.getByText('1/1')).toBeTruthy());
    expect(screen.queryByText('3/1')).toBeNull();
  });

  it('reports no matches for a query that is absent', async () => {
    mockLines['flt-3'] = ['[12:00:00] [Server thread/INFO]: hello world'];
    mockRunning['flt-3'] = true;

    render(ServerConsole, { props: { serverId: 'flt-3' } });

    const search = screen.getByTestId('server-console-search') as HTMLInputElement;
    await fireEvent.input(search, { target: { value: 'zzzzz' } });

    await waitFor(() => expect(screen.getByTestId('server-console-no-matches')).toBeTruthy());
  });
});
