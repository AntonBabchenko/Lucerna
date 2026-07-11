import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ServerWithStatus_Serialize } from '$lib/ipc/bindings';

const { serverList, serverStart, serverStop, serverDiagnose, serverUpload, getDataLocation } =
  vi.hoisted(() => ({
    serverList: vi.fn(),
    serverStart: vi.fn(),
    serverStop: vi.fn(),
    serverDiagnose: vi.fn(),
    serverUpload: vi.fn(),
    getDataLocation: vi.fn(),
  }));
vi.mock('$lib/ipc/bindings', () => ({
  commands: { serverList, serverStart, serverStop, serverDiagnose, serverUpload, getDataLocation },
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
import { dataLocation } from '$lib/settings/data-location.svelte';

function dataLocationStatus(fellBack: boolean) {
  return {
    status: 'ok' as const,
    data: {
      effective: 'C:\\Users\\test\\AppData\\Roaming\\com.lucerna.app',
      configured: fellBack ? 'D:\\LucernaData' : null,
      fell_back: fellBack,
      data_size_bytes: 0,
    },
  };
}

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
    serversUi.activeTab = 'overview';
    serversUi.creating = false;
    serverList.mockReset();
    serverStart.mockReset();
    serverStop.mockReset();
    serverDiagnose.mockReset();
    serverUpload.mockReset();
    serverDiagnose.mockResolvedValue({ status: 'error', error: { kind: 'x' } });
    // localStorage.clear() runs LAST so the reset serversUi calls above don't
    // race a still-persisted selection from a previous test.
    localStorage.clear();
  });

  // MUST stay the first test in this file: it asserts the pre-settled-load
  // state, and listLoadedOnce is module-singleton state that any load() from
  // an earlier test would have already latched true.
  it('shows neither the empty-state text nor the select before the first load settles', () => {
    render(ServerSidebarSection);
    expect(screen.queryByText('No servers yet.')).toBeNull();
    expect(screen.queryByTestId('sidebar-server-select')).toBeNull();
    expect(screen.getByTestId('sidebar-create-server')).toBeTruthy();
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

  it('switches to the Overview tab immediately on Start click, before the command resolves', async () => {
    await load([makeServer('a', false)]);
    serversUi.selectServer('a');
    serversUi.activeTab = 'settings';

    let resolveStart: ((v: { status: 'ok'; data: number }) => void) | undefined;
    const pending = new Promise((resolve) => {
      resolveStart = resolve;
    });
    serverStart.mockReturnValue(pending);

    render(ServerSidebarSection);
    await fireEvent.click(screen.getByTestId('sidebar-server-start'));

    expect(serversUi.activeTab).toBe('overview');
    expect(serverStart).toHaveBeenCalledWith('a');

    resolveStart?.({ status: 'ok', data: 1 });
    await pending;
    // Settle: runLifecycle still refreshes + clears busy after the command
    // resolves; don't leak that in-flight work into the next test. (waitFor
    // retries on THROW, not on a falsy return — hence the expect form.)
    await vi.waitFor(() => {
      expect(serverState.actionFor('a')).toBeNull();
    });
  });

  it('Start is disabled while a hosting upload is in flight (was the manage-header guard)', async () => {
    await load([makeServer('a', false)]);
    serversUi.selectServer('a');
    // Kick off an upload that stays pending so the phase remains 'uploading'.
    let resolveUpload: ((v: { status: 'ok'; data: null }) => void) | undefined;
    serverUpload.mockReturnValue(
      new Promise((resolve) => {
        resolveUpload = resolve;
      }),
    );
    const uploadDone = serverState.upload('a', false, false);

    render(ServerSidebarSection);
    const startBtn = screen.getByTestId('sidebar-server-start') as HTMLButtonElement;
    expect(startBtn.disabled).toBe(true);

    // Finish the upload so the 'uploading' phase doesn't leak into other tests.
    resolveUpload?.({ status: 'ok', data: null });
    await uploadDone;
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

  describe('data-root fallback gating (§7)', () => {
    // `dataLocation` is a module singleton — restore fell_back=false after each
    // gating test so the gate doesn't leak into the rest of the file.
    afterEach(async () => {
      getDataLocation.mockResolvedValue(dataLocationStatus(false));
      await dataLocation.refresh();
    });

    it('disables create while the data root fell back; click is a no-op', async () => {
      await load([]);
      getDataLocation.mockResolvedValue(dataLocationStatus(true));
      await dataLocation.refresh();

      render(ServerSidebarSection);
      const btn = screen.getByTestId('sidebar-create-server') as HTMLButtonElement;
      expect(btn.disabled).toBe(true);
      await fireEvent.click(btn);
      expect(serversUi.creating).toBe(false);
    });

    it('leaves create enabled when the data root is healthy', async () => {
      await load([]);
      getDataLocation.mockResolvedValue(dataLocationStatus(false));
      await dataLocation.refresh();

      render(ServerSidebarSection);
      const btn = screen.getByTestId('sidebar-create-server') as HTMLButtonElement;
      expect(btn.disabled).toBe(false);
    });
  });
});
