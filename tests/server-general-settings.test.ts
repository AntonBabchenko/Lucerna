import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import ServerSettingsTab from '$lib/servers/settings/ServerSettingsTab.svelte';

// vi.mock factories are hoisted above imports — use vi.hoisted so the shared
// mutable mock state and vi.fn() references are available inside the factory.
const { mockRename, mockUpdateRuntimeConfig, mockRunning, mockList } = vi.hoisted(() => {
  const mockRename = vi.fn().mockResolvedValue({ ok: true });
  const mockUpdateRuntimeConfig = vi.fn().mockResolvedValue({ ok: true });
  const mockRunning = vi.fn().mockReturnValue(false);
  const mockList = [
    {
      id: 'srv-1',
      name: 'Old Name',
      mc_version: '1.21.1',
      loader: 'fabric' as const,
      loader_version: null as string | null,
      max_heap_mb: 4096,
      extra_jvm_args: '',
      created_unix_ms: null as number | null,
      eula_accepted: true,
      created_from_instance: null as string | null,
      running: false,
      pid: null as number | null,
      port: null as number | null,
      upload: null,
      upload_password_set: false,
      last_exit_code: null,
      diagnosis_status: 'none',
    },
  ];
  return { mockRename, mockUpdateRuntimeConfig, mockRunning, mockList };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    instanceMemoryBounds: vi.fn().mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    }),
    serverReadProperties: vi.fn().mockResolvedValue({ status: 'ok', data: '' }),
    serverWriteProperties: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
}));

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get list() {
      return mockList;
    },
    running: mockRunning,
    rename: mockRename,
    updateRuntimeConfig: mockUpdateRuntimeConfig,
  },
}));

describe('ServerSettingsTab launch section', () => {
  beforeAll(() => locale.set('en'));

  it('(a) Name input is pre-filled; saving calls rename and updateRuntimeConfig', async () => {
    mockRunning.mockReturnValue(false);
    mockRename.mockClear();
    mockUpdateRuntimeConfig.mockClear();

    render(ServerSettingsTab, { props: { serverId: 'srv-1' } });

    const nameInput = screen.getByLabelText('Name') as HTMLInputElement;
    expect(nameInput.value).toBe('Old Name');

    // Change the name
    await fireEvent.input(nameInput, { target: { value: 'New Name' } });

    // Click Save (the merged tab has two Save buttons — target the launch one)
    const saveBtn = screen.getByTestId('settings-launch-save');
    await fireEvent.click(saveBtn);

    expect(mockRename).toHaveBeenCalledWith('srv-1', 'New Name');
    expect(mockUpdateRuntimeConfig).toHaveBeenCalledWith('srv-1', 4096, '');
  });

  it('(c) the Saved confirmation clears once a field is edited again (#34)', async () => {
    mockRunning.mockReturnValue(false);
    mockRename.mockClear();
    mockUpdateRuntimeConfig.mockClear();

    render(ServerSettingsTab, { props: { serverId: 'srv-1' } });

    const nameInput = screen.getByLabelText('Name') as HTMLInputElement;
    await fireEvent.input(nameInput, { target: { value: 'New Name' } });
    await fireEvent.click(screen.getByTestId('settings-launch-save'));

    // "Saved" appears after a successful save.
    expect(await screen.findByText('Saved')).toBeTruthy();

    // A fresh edit must drop the stale confirmation.
    await fireEvent.input(nameInput, { target: { value: 'New Name 2' } });
    expect(screen.queryByText('Saved')).toBeNull();
  });

  it('(b) shows restart warning when the server is running', () => {
    mockRunning.mockReturnValue(true);

    render(ServerSettingsTab, { props: { serverId: 'srv-1' } });

    expect(screen.getByText('Restart the server to apply memory / JVM changes.')).toBeTruthy();
  });
});
