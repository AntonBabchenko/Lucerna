import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ServerWithStatus_Serialize } from '$lib/ipc/bindings';

const { serverList, serverStart, serverStop, serverDiagnose } = vi.hoisted(() => ({
  serverList: vi.fn(),
  serverStart: vi.fn(),
  serverStop: vi.fn(),
  serverDiagnose: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: { serverList, serverStart, serverStop, serverDiagnose },
  events: {
    serverLogLine: { listen: vi.fn() },
    serverSpawned: { listen: vi.fn() },
    serverExited: { listen: vi.fn() },
    serverUploadProgress: { listen: vi.fn() },
  },
}));

import ServerSidebarSection from '$lib/servers/ServerSidebarSection.svelte';
import { serverState } from '$lib/servers/server-state.svelte';
import { serversUi } from '$lib/servers/servers-ui.svelte';

function makeServer(id: string, running: boolean): ServerWithStatus_Serialize {
  return {
    id,
    name: id,
    mc_version: '1.21',
    loader: 'vanilla',
    loader_version: null,
    max_heap_mb: 2048,
    extra_jvm_args: '',
    created_unix_ms: null,
    eula_accepted: true,
    created_from_instance: null,
    running,
    pid: running ? 1 : null,
    port: null,
    upload: null,
    upload_password_set: false,
    last_exit_code: null,
    diagnosis_status: 'none',
  };
}

async function load(data: ServerWithStatus_Serialize[]) {
  serverList.mockResolvedValue({ status: 'ok', data });
  await serverState.refresh();
}

describe('ServerSidebarSection', () => {
  beforeEach(() => {
    serversUi.setMode('client');
    serversUi.selectServer(null);
    serversUi.activeTab = 'console';
    serversUi.creating = false;
    serverList.mockReset();
    serverStart.mockReset();
    serverStop.mockReset();
    serverDiagnose.mockReset();
    serverDiagnose.mockResolvedValue({ status: 'error', error: { kind: 'x' } });
    // localStorage.clear() runs LAST so the reset serversUi calls above don't
    // race a still-persisted selection from a previous test.
    localStorage.clear();
  });

  it('shows the empty state and no select/start when there are no servers', async () => {
    await load([]);
    render(ServerSidebarSection);
    expect(screen.getByText('No servers yet.')).toBeTruthy();
    expect(screen.queryByTestId('sidebar-server-select')).toBeNull();
    expect(screen.queryByTestId('sidebar-server-start')).toBeNull();
    expect(screen.getByTestId('sidebar-create-server')).toBeTruthy();
  });

  it('clicking create sets the wizard flag', async () => {
    await load([]);
    render(ServerSidebarSection);
    await fireEvent.click(screen.getByTestId('sidebar-create-server'));
    expect(serversUi.creating).toBe(true);
  });

  it('shows Start (success, lg) for a stopped selected server, no Stop', async () => {
    await load([makeServer('a', false)]);
    serversUi.selectServer('a');
    render(ServerSidebarSection);
    const startBtn = screen.getByTestId('sidebar-server-start');
    expect(startBtn).toHaveBtnVariant('success');
    expect(startBtn).toHaveBtnSize('lg');
    expect(screen.queryByTestId('sidebar-server-stop')).toBeNull();
  });

  it('switches to the console tab immediately on Start click, before the command resolves', async () => {
    await load([makeServer('a', false)]);
    serversUi.selectServer('a');
    serversUi.activeTab = 'general';

    let resolveStart: ((v: { status: 'ok'; data: number }) => void) | undefined;
    const pending = new Promise((resolve) => {
      resolveStart = resolve;
    });
    serverStart.mockReturnValue(pending);

    render(ServerSidebarSection);
    await fireEvent.click(screen.getByTestId('sidebar-server-start'));

    expect(serversUi.activeTab).toBe('console');
    expect(serverStart).toHaveBeenCalledWith('a');

    resolveStart?.({ status: 'ok', data: 1 });
    await pending;
  });

  it('shows Stop (danger, lg) for a running selected server, no Start', async () => {
    await load([makeServer('a', true)]);
    serversUi.selectServer('a');
    render(ServerSidebarSection);
    const stopBtn = screen.getByTestId('sidebar-server-stop');
    expect(stopBtn).toHaveBtnVariant('danger');
    expect(stopBtn).toHaveBtnSize('lg');
    expect(screen.queryByTestId('sidebar-server-start')).toBeNull();

    serverStop.mockResolvedValue({ status: 'ok', data: null });
    // Stop triggers a refresh() — re-mock the list so it doesn't hang/reject.
    serverList.mockResolvedValue({ status: 'ok', data: [makeServer('a', false)] });
    await fireEvent.click(stopBtn);
    expect(serverStop).toHaveBeenCalledWith('a');
  });
});
