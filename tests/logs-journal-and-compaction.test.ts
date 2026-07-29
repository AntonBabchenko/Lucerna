// LogsPopover integration for Session 6: the Logs | History view switch, and
// the repeat-collapse row rendered by the log body.
//
// render.ts's collapse logic is unit-tested in tests/logs-render.test.ts; this
// file covers only what the component does with it.

import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LogFileMeta } from '$lib/ipc/bindings';

const SPAM = [
  '[12:04:00] [main/INFO]: before the wall',
  '[12:04:01] [Server thread/WARN]: Cannot keep up!',
  '[12:04:02] [Server thread/WARN]: Cannot keep up!',
  '[12:04:03] [Server thread/WARN]: Cannot keep up!',
  '[12:04:04] [Server thread/WARN]: Cannot keep up!',
  '[12:04:10] [main/INFO]: after the wall',
].join('\n');

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    listLogFiles: vi.fn(),
    readLogFile: vi.fn(),
    diagnoseLog: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    annotateLogFile: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { annotations: [], patterns: [] } }),
    diagnoseLatest: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    shareLogToMclogs: vi.fn(),
    latestCrash: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    openLogFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    applyLogRetention: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { deleted_count: 0, freed_bytes: 0 } }),
    deleteLogFile: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    clearOldLogs: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { deleted_count: 0, freed_bytes: 0 } }),
    instanceJournalRead: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    instanceJournalClear: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
  events: {
    processExited: { listen: vi.fn().mockResolvedValue(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import { commands } from '$lib/ipc/bindings';
import LogsPopover from '$lib/logs/LogsPopover.svelte';

const FILE: LogFileMeta = {
  path: 'C:/i/.minecraft/logs/latest.log',
  name: 'latest.log',
  source: 'game',
  size_bytes: 1024,
  modified_unix_ms: Date.UTC(2026, 6, 30, 12, 0, 0),
};

const props = {
  open: true,
  instanceId: 'inst-1',
  instanceName: 'Test',
  mcVersion: '1.21.1',
  loader: 'fabric' as const,
  gameRunning: false,
};

beforeEach(() => {
  localStorage.clear();
  vi.mocked(commands.listLogFiles).mockResolvedValue({
    status: 'ok',
    data: [FILE],
    // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
  } as any);
  vi.mocked(commands.readLogFile).mockResolvedValue({
    status: 'ok',
    data: SPAM,
    // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
  } as any);
});

afterEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
});

describe('Logs | History view switch', () => {
  it('starts on Logs and does not mount the journal panel', async () => {
    const { queryByTestId, findByText } = render(LogsPopover, { props });
    await findByText('latest.log');
    expect(queryByTestId('journal-panel')).toBeNull();
  });

  it('switching to History replaces the file list with the journal panel', async () => {
    const { findByTestId, getByTestId, queryByText, queryByTestId } = render(LogsPopover, {
      props,
    });
    await findByTestId('logs-view-journal');

    await fireEvent.click(getByTestId('logs-view-journal'));

    expect(await findByTestId('journal-panel')).toBeTruthy();
    // The log-file sidebar is gone, so its rows must be too.
    await waitFor(() => expect(queryByText('latest.log')).toBeNull());
    // …and so are the renderer-only toolbar controls.
    expect(queryByText('Collapse repeats')).toBeNull();
    // The log-file eraser must not sit next to the journal's own clear button.
    expect(queryByTestId('clear-old-logs')).toBeNull();
    expect(await findByTestId('journal-clear')).toBeTruthy();
  });
});

describe('repeat collapse in the log body', () => {
  it('collapses the spam run into one row with a ×N chip', async () => {
    const { findByText, getAllByTestId } = render(LogsPopover, { props });
    await fireEvent.click(await findByText('latest.log'));

    await waitFor(() => expect(getAllByTestId('logs-repeat-count').length).toBe(1));
    expect(getAllByTestId('logs-repeat-count')[0].textContent).toContain('4');
  });

  it('expanding the row reveals every collapsed line', async () => {
    const { findByText, getAllByTestId, container } = render(LogsPopover, { props });
    await fireEvent.click(await findByText('latest.log'));
    await waitFor(() => expect(getAllByTestId('logs-repeat-count').length).toBe(1));

    const chip = getAllByTestId('logs-repeat-count')[0];
    const row = chip.closest('button') as HTMLButtonElement;
    expect(row.getAttribute('aria-expanded')).toBe('false');

    await fireEvent.click(row);

    await waitFor(() =>
      expect(
        container.querySelectorAll('button[aria-expanded="true"]').length,
      ).toBeGreaterThanOrEqual(1),
    );
    // All four members are now on screen (1 representative + 3 revealed).
    const occurrences = (container.textContent ?? '').split('Cannot keep up!').length - 1;
    expect(occurrences).toBe(4);
  });

  it('turning the toggle off renders every line separately', async () => {
    const { findByText, getAllByTestId, getByTestId, queryAllByTestId, container } = render(
      LogsPopover,
      { props },
    );
    await fireEvent.click(await findByText('latest.log'));
    await waitFor(() => expect(getAllByTestId('logs-repeat-count').length).toBe(1));

    await fireEvent.click(getByTestId('logs-collapse-repeats'));

    await waitFor(() => expect(queryAllByTestId('logs-repeat-count').length).toBe(0));
    const occurrences = (container.textContent ?? '').split('Cannot keep up!').length - 1;
    expect(occurrences).toBe(4);
  });

  it('a collapsed row still carries its inline hint badge', async () => {
    // The whole point of keeping `index` on a repeat unit: the lines that
    // repeat are exactly the lines most likely to be annotated, so folding them
    // must not make the hint unreachable.
    vi.mocked(commands.annotateLogFile).mockResolvedValue({
      status: 'ok',
      data: {
        // Line 1 is the first member of the spam run.
        annotations: [{ line: 1, pattern_id: 'server-cant-keep-up' }],
        patterns: [
          {
            id: 'server-cant-keep-up',
            title: 'The server is falling behind',
            explanation: 'e',
            recommendation: 'r',
          },
        ],
      },
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);

    const { findByText, getAllByTestId, container } = render(LogsPopover, { props });
    await fireEvent.click(await findByText('latest.log'));
    await waitFor(() => expect(getAllByTestId('logs-repeat-count').length).toBe(1));

    const chip = getAllByTestId('logs-repeat-count')[0];
    const row = chip.closest('div.log-row') as HTMLElement;
    await waitFor(() => expect(row.querySelector('.log-hint-badge')).not.toBeNull());
    // Exactly one badge: the annotation belongs to the run's representative, and
    // the collapsed members must not each sprout their own.
    expect(container.querySelectorAll('.log-hint-badge').length).toBe(1);
  });

  it('search still finds and counts the collapsed representative', async () => {
    const { findByText, getAllByTestId, getByPlaceholderText } = render(LogsPopover, { props });
    await fireEvent.click(await findByText('latest.log'));
    await waitFor(() => expect(getAllByTestId('logs-repeat-count').length).toBe(1));

    await fireEvent.input(getByPlaceholderText(/find in file/i), {
      target: { value: 'keep up' },
    });

    // One match, not four: hidden repeat members are not searched, exactly as
    // hidden stack-fold frames are not.
    expect(await findByText('1 / 1')).toBeTruthy();
  });
});
