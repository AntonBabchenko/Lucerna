import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerLogInfo } from '$lib/ipc/bindings';
import ServerConsole from '$lib/servers/ServerConsole.svelte';

// ---------------------------------------------------------------------------
// Mock factory fns — declared via vi.hoisted so they are available inside
// the vi.mock factory callback (Vitest hoists vi.mock calls to top of file).
// ---------------------------------------------------------------------------
const { mockOpenLogsFolder, mockListLogs, mockReadLog } = vi.hoisted(() => ({
  mockOpenLogsFolder: vi.fn().mockResolvedValue(undefined),
  mockListLogs: vi.fn(),
  mockReadLog: vi.fn(),
}));

// Mock bindings — no IPC calls in unit tests.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverSendCommand: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    serverListLogs: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    serverReadLog: vi.fn().mockResolvedValue({ status: 'ok', data: '' }),
    serverOpenLogsFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
}));

// Mutable test state controlled per-test.
const mockLinesMap: Record<string, string[]> = {};
const mockRunningMap: Record<string, boolean> = {};

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get list() {
      return [];
    },
    lines: (id: string) => mockLinesMap[id] ?? [],
    running: (id: string) => mockRunningMap[id] ?? false,
    refresh: vi.fn().mockResolvedValue(undefined),
    clearLines: vi.fn(),
    init: vi.fn(),
    listLogs: (...args: unknown[]) => mockListLogs(...args),
    readLog: (...args: unknown[]) => mockReadLog(...args),
    openLogsFolder: (...args: unknown[]) => mockOpenLogsFolder(...args),
  },
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const LATEST_INFO: ServerLogInfo = {
  file_name: 'server-latest.log',
  modified_unix_ms: 1_750_000_000_000,
  size_bytes: 1024,
  is_latest: true,
};

const ARCHIVE_INFO: ServerLogInfo = {
  file_name: 'server-20260619-120000.log',
  modified_unix_ms: 1_749_990_000_000,
  size_bytes: 512,
  is_latest: false,
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('ServerConsole — log controls', () => {
  beforeAll(() => locale.set('en'));

  it('renders an "Open logs folder" button that calls openLogsFolder', async () => {
    mockLinesMap['srv-1'] = [];
    mockRunningMap['srv-1'] = false;
    mockListLogs.mockResolvedValue({ ok: true, list: [LATEST_INFO] });

    render(ServerConsole, { props: { serverId: 'srv-1' } });

    const btn = await screen.findByRole('button', { name: 'Open logs folder' });
    expect(btn).toBeTruthy();

    await fireEvent.click(btn);

    expect(mockOpenLogsFolder).toHaveBeenCalledWith('srv-1');
  });

  it('past-sessions picker lists an archived log entry', async () => {
    mockLinesMap['srv-2'] = [];
    mockRunningMap['srv-2'] = false;
    mockListLogs.mockResolvedValue({ ok: true, list: [LATEST_INFO, ARCHIVE_INFO] });

    render(ServerConsole, { props: { serverId: 'srv-2' } });

    // The custom Select renders a role=combobox trigger. Wait for it to appear
    // (the logs load asynchronously in $effect).
    const trigger = await screen.findByRole('combobox');
    expect(trigger).toBeTruthy();

    // Open the listbox by clicking the trigger.
    await fireEvent.click(trigger);

    // The archive file name should appear as an option.
    const option = screen.getByRole('option', { name: ARCHIVE_INFO.file_name });
    expect(option).toBeTruthy();
  });

  it('selecting an archived entry calls readLog and shows its text read-only', async () => {
    mockLinesMap['srv-3'] = [];
    mockRunningMap['srv-3'] = false;
    mockListLogs.mockResolvedValue({ ok: true, list: [LATEST_INFO, ARCHIVE_INFO] });
    mockReadLog.mockResolvedValue({ ok: true, text: '[INFO] old session line' });

    render(ServerConsole, { props: { serverId: 'srv-3' } });

    // Open the picker.
    const trigger = await screen.findByRole('combobox');
    await fireEvent.click(trigger);

    // Select the archive option via mousedown (the custom Select commits on mousedown).
    const option = screen.getByRole('option', { name: ARCHIVE_INFO.file_name });
    await fireEvent.mouseDown(option);

    // readLog must have been called with the chosen file.
    await waitFor(() => expect(mockReadLog).toHaveBeenCalledWith('srv-3', ARCHIVE_INFO.file_name));

    // The archived text must be displayed.
    await waitFor(() => expect(screen.getByText('[INFO] old session line')).toBeTruthy());

    // The "Back to live console" affordance must be present.
    expect(screen.getByRole('button', { name: 'Back to live console' })).toBeTruthy();
  });

  it('Back to live console returns to the live stream view', async () => {
    mockLinesMap['srv-4'] = ['[INFO] live line'];
    mockRunningMap['srv-4'] = true;
    mockListLogs.mockResolvedValue({ ok: true, list: [LATEST_INFO, ARCHIVE_INFO] });
    mockReadLog.mockResolvedValue({ ok: true, text: '[INFO] archived text' });

    render(ServerConsole, { props: { serverId: 'srv-4' } });

    // Open the picker and switch to the archive.
    const trigger = await screen.findByRole('combobox');
    await fireEvent.click(trigger);
    const option = screen.getByRole('option', { name: ARCHIVE_INFO.file_name });
    await fireEvent.mouseDown(option);

    await waitFor(() => expect(screen.getByText('[INFO] archived text')).toBeTruthy());

    // Press "Back to live console".
    const backBtn = screen.getByRole('button', { name: 'Back to live console' });
    await fireEvent.click(backBtn);

    // Live line must be visible again and archive text gone.
    await waitFor(() => expect(screen.getByText('[INFO] live line')).toBeTruthy());
    expect(screen.queryByText('[INFO] archived text')).toBeNull();
  });
});
