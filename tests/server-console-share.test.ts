import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import ServerConsole from '$lib/servers/ServerConsole.svelte';

const serverReadLog = vi.fn().mockResolvedValue({
  status: 'ok',
  data: 'crash log line\nanother line',
});
const shareLogToMclogs = vi.fn().mockResolvedValue({
  status: 'ok',
  data: 'https://mclo.gs/AbCdEf',
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverReadLog: (...a: unknown[]) => serverReadLog(...a),
    shareLogToMclogs: (...a: unknown[]) => shareLogToMclogs(...a),
    serverSendCommand: vi.fn(),
    annotateLogText: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { annotations: [], patterns: [] } }),
  },
}));

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    lines: () => [],
    running: () => false,
    listLogs: vi.fn().mockResolvedValue({ ok: true, list: [] }),
    openLogsFolder: vi.fn(),
  },
}));

const { pushSuccessMock } = vi.hoisted(() => ({ pushSuccessMock: vi.fn() }));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: pushSuccessMock,
  pushError: vi.fn(),
}));

describe('ServerConsole share', () => {
  beforeAll(() => locale.set('en'));

  it('uploads the latest log to mclo.gs and shows the shareable link', async () => {
    render(ServerConsole, { serverId: 'srv-1' });

    await fireEvent.click(screen.getByTestId('server-log-share'));

    // The returned URL is surfaced for manual copy (findByTestId waits for the
    // reactive flush after the upload resolves).
    const urlEl = await screen.findByTestId('server-log-share-url');
    expect(urlEl.textContent).toContain('https://mclo.gs/AbCdEf');
    // On the live view it reads the latest log, then shares its content.
    expect(serverReadLog).toHaveBeenCalledWith('srv-1', 'server-latest.log');
    expect(shareLogToMclogs).toHaveBeenCalledWith('crash log line\nanother line');
    expect(pushSuccessMock).toHaveBeenCalled();
  });
});
