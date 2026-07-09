import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const { serverSwitchCore, serverBackupCreate, pushSuccess, refresh } = vi.hoisted(() => ({
  serverSwitchCore: vi.fn(),
  serverBackupCreate: vi.fn(),
  pushSuccess: vi.fn(),
  refresh: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverSwitchCore: (...args: unknown[]) => serverSwitchCore(...args),
    // Present so a test can assert the modal never calls it — the backend
    // owns the pre-switch backup (server_switch_core takes its own).
    serverBackupCreate: (...args: unknown[]) => serverBackupCreate(...args),
  },
}));

vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: (...a: unknown[]) => pushSuccess(...a),
}));

vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: { refresh: (...a: unknown[]) => refresh(...a) },
}));

vi.mock('$lib/i18n', () => ({
  t: {
    subscribe: (run: (v: (key: string, values?: Record<string, unknown>) => string) => void) => {
      run((key, values) => (values ? `${key}:${JSON.stringify(values)}` : key));
      return () => {};
    },
  },
}));

import SwitchCoreModal from '$lib/servers/SwitchCoreModal.svelte';

beforeEach(() => {
  serverSwitchCore.mockReset();
  serverBackupCreate.mockReset();
  pushSuccess.mockReset();
  refresh.mockReset();
});

describe('SwitchCoreModal', () => {
  it('defaults the target to the first allowed switch target', () => {
    render(SwitchCoreModal, { serverId: 'srv-1', currentCore: 'vanilla', onClose: vi.fn() });
    // vanilla -> [paper, purpur]; the trigger button shows the selected label.
    expect(screen.getByTestId('switch-core-target').textContent).toContain('Paper');
  });

  it('calls serverSwitchCore with the selected target, refreshes state, toasts, and closes', async () => {
    serverSwitchCore.mockResolvedValue({ status: 'ok', data: null });
    const onClose = vi.fn();
    render(SwitchCoreModal, { serverId: 'srv-1', currentCore: 'vanilla', onClose });

    await fireEvent.click(screen.getByText('servers.core.confirm'));

    expect(serverSwitchCore).toHaveBeenCalledWith('srv-1', 'paper');
    await waitFor(() => expect(pushSuccess).toHaveBeenCalled());
    expect(refresh).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
    // The UI must never take its own backup — server_switch_core owns it.
    expect(serverBackupCreate).not.toHaveBeenCalled();
  });

  it('shows an inline error and does not refresh or close when the command fails', async () => {
    serverSwitchCore.mockResolvedValue({ status: 'error', error: 'boom' });
    const onClose = vi.fn();
    render(SwitchCoreModal, { serverId: 'srv-1', currentCore: 'purpur', onClose });

    await fireEvent.click(screen.getByText('servers.core.confirm'));

    await waitFor(() => expect(screen.getByTestId('switch-core-error')).toBeTruthy());
    expect(pushSuccess).not.toHaveBeenCalled();
    expect(refresh).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    expect(serverBackupCreate).not.toHaveBeenCalled();
  });

  it('never calls serverBackupCreate regardless of outcome', async () => {
    serverSwitchCore.mockResolvedValue({ status: 'ok', data: null });
    render(SwitchCoreModal, { serverId: 'srv-1', currentCore: 'paper', onClose: vi.fn() });

    await fireEvent.click(screen.getByText('servers.core.confirm'));
    await waitFor(() => expect(serverSwitchCore).toHaveBeenCalled());

    expect(serverBackupCreate).not.toHaveBeenCalled();
  });
});
