import { render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import ServerConsole from '$lib/servers/ServerConsole.svelte';

const serverReadLog = vi.fn().mockResolvedValue({
  status: 'ok',
  data: 'SAVED-LOG: server started while launcher was elsewhere',
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverReadLog: (...a: unknown[]) => serverReadLog(...a),
    serverSendCommand: vi.fn(),
    shareLogToMclogs: vi.fn().mockResolvedValue({ status: 'ok', data: 'https://mclo.gs/x' }),
  },
}));

// Running server with NO captured live output — the reopened-panel / restarted-
// launcher case where stdout isn't being streamed.
vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    lines: () => [],
    running: () => true,
    listLogs: vi.fn().mockResolvedValue({ ok: true, list: [] }),
    openLogsFolder: vi.fn(),
  },
}));

vi.mock('$lib/toasts/toasts.svelte', () => ({ pushSuccess: vi.fn(), pushError: vi.fn() }));

describe('ServerConsole backfill', () => {
  beforeAll(() => locale.set('en'));

  it('shows the saved server-latest.log when there is no live output', async () => {
    render(ServerConsole, { serverId: 'srv-1' });
    // The live console backfills from the latest log so a running server isn't blank.
    expect(await screen.findByText(/SAVED-LOG: server started/)).toBeTruthy();
    expect(serverReadLog).toHaveBeenCalledWith('srv-1', 'server-latest.log');
  });
});
