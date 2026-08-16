import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerWithStatus } from '$lib/ipc/bindings';
import ServerHostingTab from '$lib/servers/ServerHostingTab.svelte';

// Mock bindings — IPC must not fire in unit tests. The hosting tab calls these
// new commands directly (auth method, host-key preview, backup policy).
const getUploadAuthMock = vi
  .fn()
  .mockResolvedValue({ status: 'ok', data: { method: 'password', private_key_path: null } });
const setUploadAuthMock = vi.fn().mockResolvedValue({ status: 'ok', data: null });
const hostKeyPreviewMock = vi
  .fn()
  .mockResolvedValue({ status: 'ok', data: { fingerprint: 'ab:cd', trusted: false } });
const backupPolicyGetMock = vi.fn().mockResolvedValue({
  status: 'ok',
  data: { enabled: false, interval_minutes: 0, last_run_unix_ms: 0 },
});
const backupPolicySetMock = vi.fn().mockResolvedValue({ status: 'ok', data: null });

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverGetUploadAuth: (...a: unknown[]) => getUploadAuthMock(...a),
    serverSetUploadAuth: (...a: unknown[]) => setUploadAuthMock(...a),
    serverHostKeyPreview: (...a: unknown[]) => hostKeyPreviewMock(...a),
    serverBackupPolicyGet: (...a: unknown[]) => backupPolicyGetMock(...a),
    serverBackupPolicySet: (...a: unknown[]) => backupPolicySetMock(...a),
  },
  events: {},
}));

// Mock plugin-dialog so save()/open() never open a native dialog.
const saveMock = vi.fn().mockResolvedValue('/tmp/server.zip');
const openMock = vi.fn().mockResolvedValue('/home/me/.ssh/id_ed25519');
vi.mock('@tauri-apps/plugin-dialog', () => ({
  save: (...args: unknown[]) => saveMock(...args),
  open: (...args: unknown[]) => openMock(...args),
}));

// Mock toasts.
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
}));

// Mutable serverState — tests mutate these directly.
const setUploadConfigMock = vi.fn().mockResolvedValue({ status: 'ok', data: null });
const uploadMock = vi.fn().mockResolvedValue({ status: 'ok', data: null });
const uploadPreflightMock = vi.fn().mockResolvedValue(null);
const cancelUploadMock = vi.fn().mockResolvedValue({ status: 'ok', data: null });
const exportZipMock = vi.fn().mockResolvedValue({ status: 'ok', data: null });
const refreshMock = vi.fn().mockResolvedValue(undefined);

let mockList: ServerWithStatus[] = [];
let mockRunning = false;
let mockProgress: { done: number; total: number; file: string } | undefined;
let mockUploadState: import('$lib/servers/server-state.svelte').UploadState | undefined;
let mockUploading = false;
let mockLastUpload: import('$lib/ipc/bindings').LastUpload | null = null;
let mockResumeInfo: {
  resumable: boolean;
  filesTotal: number;
  filesDone: number;
  bytesTotal: number;
} | null = null;

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get list() {
      return mockList;
    },
    running: (_id: string) => mockRunning,
    uploadProgressFor: (_id: string) => mockProgress,
    clearUploadProgress: vi.fn(),
    uploadStateFor: (_id: string) => mockUploadState,
    isUploading: (_id: string) => mockUploading,
    cancelUpload: (...args: unknown[]) => cancelUploadMock(...args),
    setUploadConfig: (...args: unknown[]) => setUploadConfigMock(...args),
    upload: (...args: unknown[]) => uploadMock(...args),
    uploadResumeState: (_id: string) => Promise.resolve(mockResumeInfo),
    uploadPreflight: (...args: unknown[]) => uploadPreflightMock(...args),
    lastUploadFor: (_id: string) => mockLastUpload,
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
    last_exit_code: null,
    diagnosis_status: 'none',
    ...overrides,
  };
}

const savedUpload = {
  host: 'myhost.com',
  port: 2222,
  user: 'alice',
  remote_path: '/srv/mc',
  known_host_fp: 'trusted-fp',
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// Let the async onMount loads (serverGetUploadAuth + serverBackupPolicyGet) and
// their state updates settle before interacting. Without this, the mocks resolve
// on a microtask AFTER a synchronous click and overwrite the user's change (a
// race that only exists in tests — in the app the IPC load finishes long before
// the user touches the radio/checkbox). One macrotask drains the microtask queue.
const settle = () => new Promise((r) => setTimeout(r, 0));

describe('ServerHostingTab', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    mockList = [];
    mockRunning = false;
    mockProgress = undefined;
    mockUploadState = undefined;
    mockUploading = false;
    mockLastUpload = null;
    mockResumeInfo = null;
    uploadMock.mockClear();
    uploadMock.mockResolvedValue({ status: 'ok', data: null });
    uploadPreflightMock.mockResolvedValue(null);
    cancelUploadMock.mockResolvedValue({ status: 'ok', data: null });
    // Call history for the onMount reads and the two writes they gate: these
    // mocks are module-scope and would otherwise carry a previous test's calls
    // into a `not.toHaveBeenCalled()` assertion.
    getUploadAuthMock.mockClear();
    setUploadAuthMock.mockClear();
    setUploadConfigMock.mockClear();
    backupPolicyGetMock.mockClear();
    backupPolicySetMock.mockClear();
  });

  it('renders host and user fields', () => {
    mockList = [makeServer()];
    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    expect(screen.getByLabelText('Host')).toBeTruthy();
    expect(screen.getByLabelText('User')).toBeTruthy();
    // "Password" also names the auth-method radio, so scope to the field input.
    expect(screen.getByLabelText('Password', { selector: 'input[type="password"]' })).toBeTruthy();
    expect(screen.getByLabelText('Remote folder')).toBeTruthy();
  });

  it('seeds form fields from existing upload config', () => {
    mockList = [makeServer({ upload: savedUpload, upload_password_set: true })];
    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    expect((screen.getByLabelText('Host') as HTMLInputElement).value).toBe('myhost.com');
    expect((screen.getByLabelText('User') as HTMLInputElement).value).toBe('alice');
    expect((screen.getByLabelText('Remote folder') as HTMLInputElement).value).toBe('/srv/mc');
    expect(screen.getByText('Password saved')).toBeTruthy();
  });

  it('calls setUploadConfig + setUploadAuth with form values on Save', async () => {
    mockList = [makeServer()];
    setUploadConfigMock.mockResolvedValue({ status: 'ok', data: null });

    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    await fireEvent.input(screen.getByLabelText('Host'), { target: { value: 'example.com' } });
    await fireEvent.input(screen.getByLabelText('User'), { target: { value: 'bob' } });
    await fireEvent.input(screen.getByLabelText('Port'), { target: { value: '2222' } });
    await fireEvent.input(screen.getByLabelText('Remote folder'), {
      target: { value: '/home/bob/mc' },
    });

    await fireEvent.click(screen.getByText('Save'));

    expect(setUploadAuthMock).toHaveBeenCalledWith(
      'srv-1',
      expect.objectContaining({ method: 'password' }),
    );
    expect(setUploadConfigMock).toHaveBeenCalledWith(
      'srv-1',
      expect.objectContaining({ host: 'example.com', user: 'bob', remote_path: '/home/bob/mc' }),
      null,
    );
  });

  // ── #25 validation ─────────────────────────────────────────────────────────

  it('disables Save and Upload until host and user are filled', async () => {
    mockList = [makeServer()];
    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    const saveBtn = screen.getByText('Save').closest('button') as HTMLButtonElement;
    const uploadBtn = screen.getByText('Upload to host').closest('button') as HTMLButtonElement;
    expect(saveBtn.disabled).toBe(true);
    expect(uploadBtn.disabled).toBe(true);

    await fireEvent.input(screen.getByLabelText('Host'), { target: { value: 'h' } });
    await fireEvent.input(screen.getByLabelText('User'), { target: { value: 'u' } });
    expect(saveBtn.disabled).toBe(false);
    expect(uploadBtn.disabled).toBe(false);
  });

  // ── #16 export running-gate ─────────────────────────────────────────────────

  it('disables Export and shows a hint while the server is running', () => {
    mockList = [makeServer({ running: true })];
    mockRunning = true;
    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    const exportBtn = screen.getByText('Export .zip').closest('button') as HTMLButtonElement;
    expect(exportBtn.disabled).toBe(true);
    expect(screen.getByText(/Stop the server first — exporting a running world/)).toBeTruthy();
  });

  it('disables Upload button and shows hint when server is running', () => {
    mockList = [makeServer({ running: true, upload: savedUpload })];
    mockRunning = true;
    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    const uploadBtn = screen.getByText('Upload to host').closest('button') as HTMLButtonElement;
    expect(uploadBtn.disabled).toBe(true);
    expect(screen.getByText('Stop the server before uploading')).toBeTruthy();
  });

  // ── #24 host-key verification ───────────────────────────────────────────────

  it('shows the fingerprint to verify on first connect (no trusted key yet)', async () => {
    // upload config saved but no known_host_fp → first connect.
    mockList = [makeServer({ upload: { ...savedUpload, known_host_fp: null } })];
    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    await fireEvent.click(screen.getByText('Upload to host'));

    await waitFor(() => expect(screen.getByTestId('host-key-confirm')).toBeTruthy());
    expect(hostKeyPreviewMock).toHaveBeenCalledWith('srv-1');
    expect(screen.getByText('Verify the host key')).toBeTruthy();
    expect(screen.getByText('ab:cd')).toBeTruthy();
    // The real upload only happens after the user clicks Trust & upload.
    expect(uploadMock).not.toHaveBeenCalled();
  });

  it('shows host-key confirm when upload returns sftp_host_key_mismatch', async () => {
    // A trusted key exists → upload runs directly; the server returns a mismatch.
    mockList = [makeServer({ upload: savedUpload })];
    uploadMock.mockResolvedValue({
      status: 'error',
      error: { kind: 'sftp_host_key_mismatch', expected: 'fp1', got: 'fp2' },
    });

    render(ServerHostingTab, { props: { serverId: 'srv-1' } });
    await fireEvent.click(screen.getByText('Upload to host'));

    await waitFor(() => expect(screen.getByTestId('host-key-confirm')).toBeTruthy());
    expect(screen.getByText('Host key changed')).toBeTruthy();
    expect(screen.getByText('fp2')).toBeTruthy(); // the new fingerprint
    expect(screen.getByText('Trust & upload')).toBeTruthy();
  });

  // ── #28 SSH-key auth ────────────────────────────────────────────────────────

  it('reveals the private-key field when SSH key auth is chosen', async () => {
    mockList = [makeServer()];
    render(ServerHostingTab, { props: { serverId: 'srv-1' } });
    await settle(); // let the onMount auth/policy load apply before interacting

    // Password mode: no key field yet.
    expect(screen.queryByLabelText('Private key file')).toBeNull();

    await fireEvent.click(screen.getByRole('radio', { name: 'SSH key' }));
    expect(screen.getByLabelText('Private key file')).toBeTruthy();
    // The password field is relabelled as the key passphrase.
    expect(screen.getByLabelText('Key passphrase (if any)')).toBeTruthy();
  });

  // ── #29 automatic backups ───────────────────────────────────────────────────

  it('saves the automatic-backup policy', async () => {
    mockList = [makeServer()];
    render(ServerHostingTab, { props: { serverId: 'srv-1' } });
    await settle(); // let the onMount policy load apply before toggling

    await fireEvent.click(screen.getByRole('checkbox', { name: 'Back up automatically' }));
    await fireEvent.click(screen.getByText('Apply'));

    expect(backupPolicySetMock).toHaveBeenCalledWith(
      'srv-1',
      expect.objectContaining({ enabled: true }),
    );
  });

  // ── store-driven upload state (Task 6) ──────────────────────────────────────

  it('reattaches in-progress upload: Upload button is disabled and progress is visible', () => {
    mockList = [makeServer({ upload: savedUpload })];
    mockUploading = true;
    mockUploadState = {
      phase: 'uploading',
      filesDone: 12,
      filesTotal: 100,
      bytesDone: 0,
      bytesTotal: 0,
      currentFile: 'mods/create-1.0.jar',
      startedAtMs: Date.now(),
    };

    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    const uploadBtn = screen.getByText(/Upload|Uploading/).closest('button') as HTMLButtonElement;
    expect(uploadBtn.disabled).toBe(true);

    // Progress bar row — file count text should be visible in the button label or
    // the progress line below it (the label uses the uploading i18n key).
    expect(uploadBtn.textContent).toBeTruthy();
    // The progress text inside the button uses {done}/{total}.
    expect(uploadBtn.textContent).toMatch(/12/);

    // Current-file line is rendered below the progress bar.
    expect(screen.getByText('mods/create-1.0.jar')).toBeTruthy();
  });

  it('shows Cancel button while uploading and calls cancelUpload on click', async () => {
    mockList = [makeServer({ upload: savedUpload })];
    mockUploading = true;
    mockUploadState = {
      phase: 'uploading',
      filesDone: 5,
      filesTotal: 50,
      bytesDone: 0,
      bytesTotal: 0,
      currentFile: 'mods/botania.jar',
      startedAtMs: Date.now(),
    };

    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    const cancelBtn = screen.getByText('Cancel upload') as HTMLButtonElement;
    expect(cancelBtn).toBeTruthy();

    await fireEvent.click(cancelBtn);
    expect(cancelUploadMock).toHaveBeenCalledWith('srv-1');
  });

  // ── bytes-driven progress bar (Task 4) ─────────────────────────────────────

  it('shows bytes-driven progress bar at 25% and progress line when bytesTotal is set', () => {
    const MB = 1024 * 1024;
    mockList = [makeServer({ upload: savedUpload })];
    mockUploading = true;
    mockUploadState = {
      phase: 'uploading',
      filesDone: 1,
      filesTotal: 4,
      bytesDone: MB,
      bytesTotal: 4 * MB,
      currentFile: 'mods/create-1.0.jar',
      startedAtMs: Date.now(),
    };

    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    // Progress bar: inner fill div should have scaleX(0.25)
    const bar = screen.getByTestId('upload-progress-bar');
    const fill = bar.querySelector('div') as HTMLElement;
    expect(fill.style.transform).toContain('scaleX(0.25)');

    // Progress line should contain MB (byte sizes are formatted in MB)
    const line = screen.getByTestId('upload-progress-line');
    expect(line.textContent).toContain('MB');

    // Current file line is still rendered
    expect(screen.getByText('mods/create-1.0.jar')).toBeTruthy();
  });

  // ── Task 5: password reveal toggle + Save-password opt-out ─────────────────

  it('reveal toggle flips aria-pressed and input type', async () => {
    mockList = [makeServer()];
    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    const toggle = screen.getByRole('button', { name: 'Reveal password' });
    expect(toggle.getAttribute('aria-pressed')).toBe('false');
    const input = screen.getByLabelText('Password', {
      selector: 'input[type="password"]',
    }) as HTMLInputElement;
    expect(input.type).toBe('password');

    await fireEvent.click(toggle);

    expect(toggle.getAttribute('aria-pressed')).toBe('true');
    expect(input.type).toBe('text');
  });

  it('save-off: setUploadConfig receives null password', async () => {
    mockList = [makeServer()];
    setUploadConfigMock.mockResolvedValue({ status: 'ok', data: null });

    render(ServerHostingTab, { props: { serverId: 'srv-1' } });
    await settle();

    // Fill host + user so Save is enabled
    await fireEvent.input(screen.getByLabelText('Host'), { target: { value: 'myhost.com' } });
    await fireEvent.input(screen.getByLabelText('User'), { target: { value: 'alice' } });
    // Type a password using the id directly to avoid radio collision
    await fireEvent.input(
      screen.getByLabelText('Password', { selector: 'input[type="password"]' }),
      { target: { value: 'secret' } },
    );
    // Uncheck "Save password"
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Save password' }));

    await fireEvent.click(screen.getByText('Save'));

    // setUploadConfig should receive null for the password (don't persist)
    expect(setUploadConfigMock).toHaveBeenCalledWith(
      'srv-1',
      expect.objectContaining({ host: 'myhost.com' }),
      null,
    );
  });

  it('save-off: upload forwards the typed password as transient', async () => {
    mockList = [makeServer({ upload: savedUpload, upload_password_set: false })];

    render(ServerHostingTab, { props: { serverId: 'srv-1' } });
    await settle();

    // Type a password and opt out of saving
    await fireEvent.input(
      screen.getByLabelText('Password', { selector: 'input[type="password"]' }),
      { target: { value: 'transient123' } },
    );
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Save password' }));

    await fireEvent.click(screen.getByText('Upload to host'));

    await waitFor(() =>
      expect(uploadMock).toHaveBeenCalledWith('srv-1', false, false, 'transient123'),
    );
  });

  it('save-off + nothing stored: Upload is disabled until password is typed', async () => {
    // No existing upload_password_set, save is off → uploadReady = false until typed
    mockList = [makeServer({ upload: savedUpload, upload_password_set: false })];

    render(ServerHostingTab, { props: { serverId: 'srv-1' } });
    await settle();

    // Uncheck Save password
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Save password' }));

    const uploadBtn = screen.getByText('Upload to host').closest('button') as HTMLButtonElement;
    expect(uploadBtn.disabled).toBe(true);

    // Type a password → upload should become enabled
    await fireEvent.input(
      screen.getByLabelText('Password', { selector: 'input[type="password"]' }),
      { target: { value: 'mypass' } },
    );
    expect(uploadBtn.disabled).toBe(false);
  });

  // ── J / K / L: skip-worlds toggle, size preflight, last-upload line ─────────

  it('passes skipWorlds=true to upload when the toggle is checked', async () => {
    // A trusted key exists → Upload runs directly through doUpload(false).
    mockList = [makeServer({ upload: savedUpload })];
    render(ServerHostingTab, { props: { serverId: 'srv-1' } });
    await settle();

    const toggle = screen.getByLabelText(/Don't upload world|Не заливать мир/) as HTMLInputElement;
    await fireEvent.click(toggle);
    expect(toggle.checked).toBe(true);

    await fireEvent.click(screen.getByRole('button', { name: /Upload to host|Залить/ }));

    // upload(id, acceptNewHostKey, skipWorlds, password) — index 2 is skipWorlds.
    await waitFor(() => expect(uploadMock).toHaveBeenCalled());
    const args = uploadMock.mock.calls.at(-1);
    expect(args?.[2]).toBe(true);
  });

  it('renders the last-upload line when the server has a last_upload', () => {
    mockList = [makeServer({ upload: savedUpload })];
    mockLastUpload = { unix_ms: 1_700_000_000_000, target: 'myhost.com:2222/srv/mc' };
    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    expect(screen.getByTestId('last-upload-line')).toBeTruthy();
    expect(screen.getByText(/Last upload|Последняя заливка/)).toBeTruthy();
  });

  it('shows the over-capacity warning when preflight exceeds free space', async () => {
    mockList = [makeServer({ upload: savedUpload })];
    uploadPreflightMock.mockResolvedValue({
      total_bytes: 100,
      free_bytes: 50,
      exceeds_free: true,
    });
    render(ServerHostingTab, { props: { serverId: 'srv-1' } });
    await settle();

    await fireEvent.click(screen.getByRole('button', { name: /Check size|Проверить размер/ }));

    expect(await screen.findByText(/won't fit|не поместятся/)).toBeTruthy();
    expect(screen.getByTestId('preflight-over')).toBeTruthy();
  });

  // ── Section B: resume after restart ─────────────────────────────────────────

  it('offers Continue when a resumable upload exists, and resumes on click', async () => {
    mockList = [makeServer({ upload: savedUpload })];
    mockResumeInfo = {
      resumable: true,
      filesTotal: 100,
      filesDone: 42,
      bytesTotal: 5_000_000,
    };
    render(ServerHostingTab, { props: { serverId: 'srv-1' } });

    // The Continue button appears (resolves the onMount async query).
    const resume = await screen.findByRole('button', { name: /Продолжить|Continue/ });
    expect(resume).toBeTruthy();

    await fireEvent.click(resume);
    // Resume calls the store upload with resume = true at position 4 (index 4),
    // robust to whatever skipWorlds/password the tab passes.
    await waitFor(() => expect(uploadMock).toHaveBeenCalled());
    expect(uploadMock.mock.calls.at(-1)?.[4]).toBe(true);
  });
  // -- failed onMount reads must not leave Save armed with guessed defaults ----

  it('surfaces a failed auth read and disables Save until a retry succeeds', async () => {
    // A trusted, fully-configured server: every other Save precondition holds,
    // so the ONLY thing that may keep Save disabled is the unread auth method.
    mockList = [makeServer({ upload: savedUpload })];
    getUploadAuthMock.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: '/servers/srv-1/upload-auth.json', details: 'permission denied' },
    });

    render(ServerHostingTab, { props: { serverId: 'srv-1' } });
    await settle();

    // The failure is on screen, not swallowed.
    expect(screen.getByTestId('hosting-auth-load-error').textContent).toContain(
      'permission denied',
    );
    const saveBtn = screen.getByText('Save').closest('button') as HTMLButtonElement;
    expect(saveBtn.disabled).toBe(true);

    // ...and nothing was written on the way there.
    expect(setUploadAuthMock).not.toHaveBeenCalled();
    expect(setUploadConfigMock).not.toHaveBeenCalled();

    // Retry succeeds -> the error clears and Save comes back.
    await fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(saveBtn.disabled).toBe(false));
    expect(screen.queryByTestId('hosting-auth-load-error')).toBeNull();
  });

  it('surfaces a failed backup-policy read and disables Apply', async () => {
    mockList = [makeServer({ upload: savedUpload })];
    backupPolicyGetMock.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: '/servers/srv-1/backup-policy.json', details: 'disk unreadable' },
    });

    render(ServerHostingTab, { props: { serverId: 'srv-1' } });
    await settle();

    expect(screen.getByTestId('hosting-backup-load-error').textContent).toContain(
      'disk unreadable',
    );
    const apply = screen.getByText('Apply').closest('button') as HTMLButtonElement;
    expect(apply.disabled).toBe(true);

    // Clicking a disabled button is a no-op; assert the write never happens, so
    // an "off, every 60 min" default can't replace the real schedule.
    await fireEvent.click(apply);
    expect(backupPolicySetMock).not.toHaveBeenCalled();
  });
});
