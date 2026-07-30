import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerWithStatus_Serialize } from '$lib/ipc/bindings';
import { serverState } from '$lib/servers/server-state.svelte';
import ServerSettingsTab from '$lib/servers/settings/ServerSettingsTab.svelte';

// vi.mock factories are hoisted above imports — use vi.hoisted so the shared
// vi.fn() references are available inside the factory (same pattern as
// server-delete.test.ts).
const {
  serverList,
  serverRename,
  serverUpdateRuntimeConfig,
  serverReadProperties,
  serverWriteProperties,
  instanceMemoryBounds,
} = vi.hoisted(() => ({
  serverList: vi.fn(),
  serverRename: vi.fn(),
  serverUpdateRuntimeConfig: vi.fn(),
  serverReadProperties: vi.fn(),
  serverWriteProperties: vi.fn(),
  instanceMemoryBounds: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverList,
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

async function seedList(data: ServerWithStatus_Serialize[]) {
  serverList.mockResolvedValue({ status: 'ok', data });
  await serverState.refresh();
}

describe('ServerSettingsTab', () => {
  beforeAll(() => locale.set('en'));

  beforeEach(() => {
    serverList.mockReset();
    serverRename.mockReset();
    serverUpdateRuntimeConfig.mockReset();
    serverReadProperties.mockReset();
    serverWriteProperties.mockReset();
    instanceMemoryBounds.mockReset();
    instanceMemoryBounds.mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      default_mb: 2048,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    });
  });

  it('the two sections have independent Saved pills', async () => {
    // Save the launch section only → exactly one "Saved" pill appears.
    serverReadProperties.mockResolvedValue({ status: 'ok', data: '' });
    serverRename.mockResolvedValue({ status: 'ok', data: makeServer('srv-1', false) });
    serverUpdateRuntimeConfig.mockResolvedValue({ status: 'ok', data: makeServer('srv-1', false) });
    await seedList([makeServer('srv-1', false)]);

    render(ServerSettingsTab, { props: { serverId: 'srv-1' } });

    await screen.findByTestId('settings-launch-save');
    await fireEvent.click(screen.getByTestId('settings-launch-save'));
    await waitFor(() => expect(screen.getAllByText('Saved')).toHaveLength(1));
  });

  it('shows the properties restart hint only while running', async () => {
    serverReadProperties.mockResolvedValue({ status: 'ok', data: '' });
    await seedList([makeServer('srv-1', true)]);

    render(ServerSettingsTab, { props: { serverId: 'srv-1' } });

    expect(
      await screen.findByText('Restart the server to apply changes to server.properties.'),
    ).toBeTruthy();
  });
});
