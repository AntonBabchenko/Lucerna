import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerWithStatus_Serialize } from '$lib/ipc/bindings';
import { serverState } from '$lib/servers/server-state.svelte';
import { serversUi } from '$lib/servers/servers-ui.svelte';
import ServerSettingsTab from '$lib/servers/settings/ServerSettingsTab.svelte';

// vi.mock factories are hoisted above imports — use vi.hoisted so the shared
// vi.fn() references are available inside the factory (same pattern as
// server-sidebar-section.test.ts).
const {
  serverList,
  serverDelete,
  serverRename,
  serverUpdateRuntimeConfig,
  serverReadProperties,
  serverWriteProperties,
  instanceMemoryBounds,
} = vi.hoisted(() => ({
  serverList: vi.fn(),
  serverDelete: vi.fn(),
  serverRename: vi.fn(),
  serverUpdateRuntimeConfig: vi.fn(),
  serverReadProperties: vi.fn().mockResolvedValue({ status: 'ok', data: '' }),
  serverWriteProperties: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  instanceMemoryBounds: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverList,
    serverDelete,
    serverRename,
    serverUpdateRuntimeConfig,
    serverReadProperties,
    serverWriteProperties,
    instanceMemoryBounds,
  },
  events: {
    serverLogLine: { listen: vi.fn().mockResolvedValue(() => {}) },
    serverSpawned: { listen: vi.fn().mockResolvedValue(() => {}) },
    serverExited: { listen: vi.fn().mockResolvedValue(() => {}) },
    serverUploadProgress: { listen: vi.fn().mockResolvedValue(() => {}) },
  },
}));

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

describe('ServerSettingsTab danger zone (delete server)', () => {
  beforeAll(() => locale.set('en'));

  beforeEach(() => {
    serverList.mockReset();
    serverDelete.mockReset();
    serverRename.mockReset();
    serverUpdateRuntimeConfig.mockReset();
    instanceMemoryBounds.mockReset();
    instanceMemoryBounds.mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      default_mb: 2048,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    });
    serversUi.selectServer(null);
    localStorage.clear();
  });

  it('(a) trigger is disabled while the server is running; confirm dialog never appears', async () => {
    await load([makeServer('a', true)]);

    render(ServerSettingsTab, { props: { serverId: 'a' } });

    const trigger = screen.getByTestId('server-delete-trigger') as HTMLButtonElement;
    expect(trigger.disabled).toBe(true);

    await fireEvent.click(trigger);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('(b) clicking the trigger on a stopped server shows the DeleteServerDialog', async () => {
    await load([makeServer('a', false)]);

    render(ServerSettingsTab, { props: { serverId: 'a' } });

    const trigger = screen.getByTestId('server-delete-trigger') as HTMLButtonElement;
    expect(trigger.disabled).toBe(false);

    await fireEvent.click(trigger);

    const dialog = screen.getByRole('dialog');
    expect(dialog).toBeTruthy();
    expect(screen.getByText('Delete server?')).toBeTruthy();
  });

  it('(c) confirming calls serverDelete and falls back selection to the next server', async () => {
    await load([makeServer('a', false), makeServer('b', false)]);
    serverDelete.mockResolvedValue({ status: 'ok', data: null });
    serversUi.selectServer('a');

    render(ServerSettingsTab, { props: { serverId: 'a' } });

    await fireEvent.click(screen.getByTestId('server-delete-trigger'));
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));

    expect(serverDelete).toHaveBeenCalledWith('a');
    expect(serversUi.selectedServerId).toBe('b');
  });

  it('(c) deleting the last remaining server falls back selection to null', async () => {
    await load([makeServer('a', false)]);
    serverDelete.mockResolvedValue({ status: 'ok', data: null });
    serversUi.selectServer('a');

    render(ServerSettingsTab, { props: { serverId: 'a' } });

    await fireEvent.click(screen.getByTestId('server-delete-trigger'));
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));

    expect(serverDelete).toHaveBeenCalledWith('a');
    expect(serversUi.selectedServerId).toBeNull();
  });

  it('(e) trigger is disabled while the remove call is pending', async () => {
    await load([makeServer('a', false)]);
    let resolveDelete: ((v: { status: 'ok'; data: null }) => void) | undefined;
    serverDelete.mockReturnValue(
      new Promise((resolve) => {
        resolveDelete = resolve;
      }),
    );
    serversUi.selectServer('a');

    render(ServerSettingsTab, { props: { serverId: 'a' } });

    await fireEvent.click(screen.getByTestId('server-delete-trigger'));
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));

    const trigger = screen.getByTestId('server-delete-trigger') as HTMLButtonElement;
    expect(trigger.disabled).toBe(true);

    resolveDelete?.({ status: 'ok', data: null });
    await vi.waitFor(() => expect(trigger.disabled).toBe(false));
  });

  it('(d) a failed remove renders the error with role="alert" and keeps the selection unchanged', async () => {
    await load([makeServer('a', false)]);
    serverDelete.mockResolvedValue({ status: 'error', error: { kind: 'io', details: 'nope' } });
    serversUi.selectServer('a');

    render(ServerSettingsTab, { props: { serverId: 'a' } });

    await fireEvent.click(screen.getByTestId('server-delete-trigger'));
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));

    expect(serverDelete).toHaveBeenCalledWith('a');
    const alert = screen.getByRole('alert');
    expect(alert.textContent).toBeTruthy();
    // A failed delete must not disturb the current selection.
    expect(serversUi.selectedServerId).toBe('a');
  });
});
