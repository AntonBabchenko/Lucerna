import { render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerWithStatus_Serialize } from '$lib/ipc/bindings';

// The Connection and Console cards are stubbed: they carry their own suites
// (server-connect-view / server-console) and would drag connectivity IPC and
// the console buffer into this one. The card grid + Server card stay real.
vi.mock('$lib/servers/ServerConsole.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/overview/ServerConnectCard.svelte', () => ({ default: stubComponent() }));

// A minimal no-op Svelte 5 component for child stubs (Svelte 5 mounts a
// component by CALLING it, so a plain function that renders nothing works).
function stubComponent() {
  return function noopComponent() {
    return {};
  };
}

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

import ServerOverviewTab from '$lib/servers/overview/ServerOverviewTab.svelte';
import { serverState } from '$lib/servers/server-state.svelte';

function makeServer(id: string): ServerWithStatus_Serialize {
  return {
    id,
    name: id,
    mc_version: '1.21',
    loader: 'fabric',
    loader_version: '0.16.0',
    max_heap_mb: 4096,
    extra_jvm_args: '',
    created_unix_ms: null,
    eula_accepted: true,
    created_from_instance: null,
    running: false,
    pid: null,
    port: null,
    upload: null,
    upload_password_set: false,
    last_exit_code: null,
    diagnosis_status: 'none',
  };
}

async function seed(data: ServerWithStatus_Serialize[]) {
  serverList.mockResolvedValue({ status: 'ok', data });
  await serverState.refresh();
}

describe('ServerOverviewTab', () => {
  beforeAll(() => locale.set('en'));

  it('shows version, core with loader version, and the heap label on the Server card', async () => {
    await seed([makeServer('srv-1')]);
    render(ServerOverviewTab, { serverId: 'srv-1' });
    const card = screen.getByTestId('server-overview-server-card');
    expect(card.textContent).toContain('1.21');
    expect(card.textContent).toContain('Fabric (0.16.0)');
    expect(card.textContent).toContain('4.0 GB');
  });

  it('renders both cards, with the console card spanning the full grid width', async () => {
    await seed([makeServer('srv-1')]);
    render(ServerOverviewTab, { serverId: 'srv-1' });
    expect(screen.getByTestId('server-overview-server-card')).toBeTruthy();
    const consoleCard = screen.getByTestId('server-overview-console-card');
    expect(consoleCard.getAttribute('style')).toContain('grid-column: 1 / -1');
  });

  it('renders no cards for an unknown serverId', async () => {
    await seed([makeServer('srv-1')]);
    render(ServerOverviewTab, { serverId: 'no-such-server' });
    expect(screen.queryByTestId('server-overview-server-card')).toBeNull();
    expect(screen.queryByTestId('server-overview-console-card')).toBeNull();
  });
});
