import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ServerWithStatus_Serialize } from '$lib/ipc/bindings';

const { serverList } = vi.hoisted(() => ({ serverList: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({
  commands: { serverList },
  events: {
    serverLogLine: { listen: vi.fn() },
    serverSpawned: { listen: vi.fn() },
    serverExited: { listen: vi.fn() },
    serverUploadProgress: { listen: vi.fn() },
  },
}));

import ModeSwitcher from '$lib/layout/ModeSwitcher.svelte';
import { serverState } from '$lib/servers/server-state.svelte';
import { serversUi } from '$lib/servers/servers-ui.svelte';

function makeServer(
  id: string,
  running: boolean,
  lastExitCode: number | null = null,
  diagnosisStatus: ServerWithStatus_Serialize['diagnosis_status'] = 'none',
): ServerWithStatus_Serialize {
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
    last_exit_code: lastExitCode,
    diagnosis_status: diagnosisStatus,
  };
}

async function load(data: ServerWithStatus_Serialize[]) {
  serverList.mockResolvedValue({ status: 'ok', data });
  await serverState.refresh();
}

describe('ModeSwitcher', () => {
  beforeEach(() => {
    serversUi.setMode('client');
    serverList.mockReset();
    localStorage.clear();
  });

  it('renders both segments with aria-pressed reflecting the mode', async () => {
    await load([]);
    render(ModeSwitcher);
    const client = screen.getByTestId('mode-switch-client');
    const servers = screen.getByTestId('mode-switch-servers');
    expect(client.getAttribute('aria-pressed')).toBe('true');
    expect(servers.getAttribute('aria-pressed')).toBe('false');
  });

  it('clicking the servers segment switches the mode', async () => {
    await load([]);
    render(ModeSwitcher);
    await fireEvent.click(screen.getByTestId('mode-switch-servers'));
    expect(serversUi.mode).toBe('servers');
    expect(localStorage.getItem('lucerna.ui.mode')).toBe('servers');
  });

  it('shows the wrench badge when a server has an actionable diagnosis', async () => {
    await load([makeServer('a', false, 1, 'actionable')]);
    render(ModeSwitcher);
    expect(screen.getByTestId('mode-servers-fix-badge')).toBeTruthy();
  });

  it('announces a running server via the status label', async () => {
    await load([makeServer('a', true)]);
    render(ModeSwitcher);
    expect(screen.getByRole('img', { name: 'Server running' })).toBeTruthy();
  });
});
