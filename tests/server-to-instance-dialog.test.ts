import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const { serverCreateClientInstance, pushSuccess, pushWarning } = vi.hoisted(() => ({
  serverCreateClientInstance: vi.fn(),
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverCreateClientInstance: (...args: unknown[]) => serverCreateClientInstance(...args),
  },
}));

vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: (...a: unknown[]) => pushSuccess(...a),
  pushWarning: (...a: unknown[]) => pushWarning(...a),
}));

vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));

vi.mock('$lib/i18n', () => ({
  t: {
    subscribe: (run: (v: (key: string, values?: Record<string, unknown>) => string) => void) => {
      run((key) => key);
      return () => {};
    },
  },
}));

import ServerToInstanceDialog from '$lib/servers/ServerToInstanceDialog.svelte';

const server = {
  id: 'srv-1',
  name: 'My SMP',
  mc_version: '1.20.4',
  loader: 'fabric' as const,
  loader_version: '0.16.5',
  max_heap_mb: 4096,
  extra_jvm_args: '',
  created_unix_ms: 1,
  eula_accepted: true,
  created_from_instance: null,
  running: false,
  pid: null,
  port: 25570,
  upload: null,
  upload_password_set: false,
};

beforeEach(() => {
  serverCreateClientInstance.mockReset();
  pushSuccess.mockReset();
  pushWarning.mockReset();
});

describe('ServerToInstanceDialog', () => {
  it('defaults the name to the server name and the checkbox to on', () => {
    render(ServerToInstanceDialog, { server, onCancel: vi.fn(), onCreated: vi.fn() });
    const nameInput = screen.getByTestId('client-instance-name') as HTMLInputElement;
    expect(nameInput.value).toBe('My SMP');
    const checkbox = screen.getByTestId('add-to-multiplayer') as HTMLInputElement;
    expect(checkbox.checked).toBe(true);
  });

  it('calls the command with the server id, name, and multiplayer flag and reports success', async () => {
    serverCreateClientInstance.mockResolvedValue({
      status: 'ok',
      data: { instance: { id: 'inst-9' }, multiplayer_added: true },
    });
    const onCreated = vi.fn();
    render(ServerToInstanceDialog, { server, onCancel: vi.fn(), onCreated });

    await fireEvent.click(screen.getByText('servers.toInstance.create'));

    expect(serverCreateClientInstance).toHaveBeenCalledWith('srv-1', 'My SMP', true);
    await waitFor(() => expect(pushSuccess).toHaveBeenCalled());
    expect(onCreated).toHaveBeenCalledWith('inst-9');
  });

  it('warns when the user opted into multiplayer but the server was not added', async () => {
    serverCreateClientInstance.mockResolvedValue({
      status: 'ok',
      data: { instance: { id: 'inst-9' }, multiplayer_added: false },
    });
    render(ServerToInstanceDialog, { server, onCancel: vi.fn(), onCreated: vi.fn() });

    await fireEvent.click(screen.getByText('servers.toInstance.create'));
    await waitFor(() => expect(pushWarning).toHaveBeenCalled());

    expect(pushSuccess).not.toHaveBeenCalled();
  });

  it('shows an inline error and does not call the command when the name is empty', async () => {
    render(ServerToInstanceDialog, {
      server: { ...server, name: '   ' },
      onCancel: vi.fn(),
      onCreated: vi.fn(),
    });
    await fireEvent.click(screen.getByText('servers.toInstance.create'));
    expect(serverCreateClientInstance).not.toHaveBeenCalled();
    expect(screen.getByTestId('client-instance-error')).toBeTruthy();
  });

  it('renders the inline error when the command returns an error', async () => {
    serverCreateClientInstance.mockResolvedValue({ status: 'error', error: 'boom' });
    const onCreated = vi.fn();
    render(ServerToInstanceDialog, { server, onCancel: vi.fn(), onCreated });
    await fireEvent.click(screen.getByText('servers.toInstance.create'));
    await waitFor(() => expect(screen.getByTestId('client-instance-error')).toBeTruthy());
    expect(onCreated).not.toHaveBeenCalled();
  });
});
