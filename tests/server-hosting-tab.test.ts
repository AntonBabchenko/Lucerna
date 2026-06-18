import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerWithStatus } from '$lib/ipc/bindings';
import ServerHostingTab from '$lib/servers/ServerHostingTab.svelte';

// Mock bindings — IPC must not fire in unit tests.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {},
  events: {},
}));

// Mock plugin-dialog so save() never opens a native dialog.
const saveMock = vi.fn().mockResolvedValue('/tmp/server.zip');
vi.mock('@tauri-apps/plugin-dialog', () => ({
  save: (...args: unknown[]) => saveMock(...args),
}));

// Mock toasts.
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
}));

// Mutable serverState — tests mutate these directly.
const setUploadConfigMock = vi.fn().mockResolvedValue({ status: 'ok', data: null });
const uploadMock = vi.fn().mockResolvedValue({ status: 'ok', data: null });
const exportZipMock = vi.fn().mockResolvedValue({ status: 'ok', data: null });
const refreshMock = vi.fn().mockResolvedValue(undefined);

let mockList: ServerWithStatus[] = [];
let mockRunning = false;
let mockProgress: { done: number; total: number; file: string } | undefined;

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get list() {
      return mockList;
    },
    running: (_id: string) => mockRunning,
    uploadProgressFor: (_id: string) => mockProgress,
    setUploadConfig: (...args: unknown[]) => setUploadConfigMock(...args),
    upload: (...args: unknown[]) => uploadMock(...args),
    exportZip: (...args: unknown[]) => exportZipMock(...args),
    refresh: () => refreshMock(),
    init: vi.fn(),
    diagnosisFor: vi.fn(),
    diagnose: vi.fn(),
  },
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeServer(overrides: Partial<ServerWithStatus> = {}): ServerWithStatus {
  return {
    id: 'srv-1',
    name: 'My Server',
    mc_version: '1.21',
    loader: 'vanilla',
    loader_version: null,
    max_heap_mb: 2048,
    extra_jvm_args: '',
    created_unix_ms: null,
    eula_accepted: true,
    created_from_instance: null,
    running: false,
    pid: null,
    port: null,
    upload: null,
    upload_password_set: false,
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('ServerHostingTab', () => {
  beforeAll(() => locale.set('en'));

  it('renders host and user fields', () => {
    mockList = [makeServer()];
    mockRunning = false;

    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    expect(screen.getByLabelText('Host')).toBeTruthy();
    expect(screen.getByLabelText('User')).toBeTruthy();
    expect(screen.getByLabelText('Password')).toBeTruthy();
    expect(screen.getByLabelText('Remote folder')).toBeTruthy();
  });

  it('seeds form fields from existing upload config', () => {
    mockList = [
      makeServer({
        upload: { host: 'myhost.com', port: 2222, user: 'alice', remote_path: '/srv/mc' },
        upload_password_set: true,
      }),
    ];
    mockRunning = false;

    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    const hostInput = screen.getByLabelText('Host') as HTMLInputElement;
    const userInput = screen.getByLabelText('User') as HTMLInputElement;
    const pathInput = screen.getByLabelText('Remote folder') as HTMLInputElement;

    expect(hostInput.value).toBe('myhost.com');
    expect(userInput.value).toBe('alice');
    expect(pathInput.value).toBe('/srv/mc');
    // "Password saved" hint visible when password_set and no password typed
    expect(screen.getByText('Password saved')).toBeTruthy();
  });

  it('calls setUploadConfig with form values on Save', async () => {
    mockList = [makeServer()];
    mockRunning = false;
    setUploadConfigMock.mockResolvedValue({ status: 'ok', data: null });

    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    const hostInput = screen.getByLabelText('Host') as HTMLInputElement;
    const userInput = screen.getByLabelText('User') as HTMLInputElement;
    const portInput = screen.getByLabelText('Port') as HTMLInputElement;
    const pathInput = screen.getByLabelText('Remote folder') as HTMLInputElement;

    await fireEvent.input(hostInput, { target: { value: 'example.com' } });
    await fireEvent.input(userInput, { target: { value: 'bob' } });
    await fireEvent.input(portInput, { target: { value: '2222' } });
    await fireEvent.input(pathInput, { target: { value: '/home/bob/mc' } });

    const saveBtn = screen.getByText('Save');
    await fireEvent.click(saveBtn);

    expect(setUploadConfigMock).toHaveBeenCalledWith(
      'srv-1',
      expect.objectContaining({
        host: 'example.com',
        user: 'bob',
        remote_path: '/home/bob/mc',
      }),
      null,
    );
  });

  it('disables Upload button and shows hint when server is running', () => {
    mockList = [makeServer({ running: true })];
    mockRunning = true;

    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    const uploadBtn = screen.getByText('Upload to host').closest('button') as HTMLButtonElement;
    expect(uploadBtn.disabled).toBe(true);
    expect(screen.getByText('Stop the server before uploading')).toBeTruthy();
  });

  it('shows host-key confirm when upload returns sftp_host_key_mismatch', async () => {
    mockList = [makeServer()];
    mockRunning = false;
    uploadMock.mockResolvedValue({
      status: 'error',
      error: { kind: 'sftp_host_key_mismatch', expected: 'fp1', got: 'fp2' },
    });

    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    const uploadBtn = screen.getByText('Upload to host').closest('button') as HTMLButtonElement;
    await fireEvent.click(uploadBtn);

    expect(screen.getByTestId('host-key-confirm')).toBeTruthy();
    expect(screen.getByText('Unrecognized host key')).toBeTruthy();
    expect(screen.getByText('Trust & upload')).toBeTruthy();
  });
});
