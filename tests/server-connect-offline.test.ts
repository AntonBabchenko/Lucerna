import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';

const { connectivity, running, firewallStatus } = vi.hoisted(() => ({
  connectivity: vi.fn(),
  running: vi.fn(),
  firewallStatus: vi.fn(),
}));
const { serverReadProperties, serverWriteProperties, serverRestart } = vi.hoisted(() => ({
  serverReadProperties: vi.fn(),
  serverWriteProperties: vi.fn(),
  serverRestart: vi.fn(),
}));

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: { connectivity, running, firewallStatus },
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverReadProperties: (...a: unknown[]) => serverReadProperties(...a),
    serverWriteProperties: (...a: unknown[]) => serverWriteProperties(...a),
    serverRestart: (...a: unknown[]) => serverRestart(...a),
  },
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({ pushSuccess: vi.fn(), pushError: vi.fn() }));

Object.defineProperty(globalThis.navigator, 'clipboard', {
  value: { writeText: vi.fn().mockResolvedValue(undefined) },
  configurable: true,
});

import ServerConnectView from '$lib/servers/ServerConnectView.svelte';

describe('ServerConnectView — allow offline players', () => {
  beforeAll(() => locale.set('en'));

  it('turns off online-mode and restarts the running server', async () => {
    running.mockReturnValue(true);
    firewallStatus.mockResolvedValue('not_applicable');
    connectivity.mockResolvedValue({ lan_addresses: [], port: 25565, online_mode: true });
    serverReadProperties.mockResolvedValue({
      status: 'ok',
      data: 'online-mode=true\nenforce-secure-profile=true\n',
    });
    serverWriteProperties.mockResolvedValue({ status: 'ok', data: null });
    serverRestart.mockResolvedValue({ status: 'ok', data: 4321 });

    render(ServerConnectView, { serverId: 'srv-1' });

    const btn = await screen.findByTestId('server-allow-offline');
    await fireEvent.click(btn);

    await vi.waitFor(() => expect(serverRestart).toHaveBeenCalledWith('srv-1'));
    expect(serverReadProperties).toHaveBeenCalledWith('srv-1');
    // Wrote online-mode=false (and disabled secure-profile enforcement).
    const written = serverWriteProperties.mock.calls[0][1] as string;
    expect(written).toContain('online-mode=false');
    expect(written).toContain('enforce-secure-profile=false');
  });

  it('does not offer the action when online-mode is already off', async () => {
    running.mockReturnValue(true);
    firewallStatus.mockResolvedValue('not_applicable');
    connectivity.mockResolvedValue({ lan_addresses: [], port: 25565, online_mode: false });
    render(ServerConnectView, { serverId: 'srv-2' });
    // Wait for the snapshot-driven explainer to appear, then assert no button.
    expect(await screen.findByText(/online-mode is OFF/i)).toBeTruthy();
    expect(screen.queryByTestId('server-allow-offline')).toBeNull();
  });
});
