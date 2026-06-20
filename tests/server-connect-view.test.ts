import { render, screen } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';

const { connectivity, running, firewallStatus } = vi.hoisted(() => ({
  connectivity: vi.fn(),
  running: vi.fn(),
  firewallStatus: vi.fn(),
}));
vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: { connectivity, running, firewallStatus },
}));

// Mock navigator.clipboard so copy-invite doesn't throw in jsdom
Object.defineProperty(globalThis.navigator, 'clipboard', {
  value: { writeText: vi.fn().mockResolvedValue(undefined) },
  configurable: true,
});

import ServerConnectView from '$lib/servers/ServerConnectView.svelte';

describe('ServerConnectView', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    connectivity.mockReset();
    running.mockReset();
    firewallStatus.mockReset();
    // Default: not_applicable so the firewall banner never appears in these tests.
    firewallStatus.mockResolvedValue('not_applicable');
  });

  it('shows a start hint when the server is stopped', async () => {
    running.mockReturnValue(false);
    connectivity.mockResolvedValue({ lan_addresses: [], port: null, online_mode: true });
    render(ServerConnectView, { serverId: 'srv-1' });
    expect(await screen.findByText('Start the server to get a join address.')).toBeTruthy();
  });

  it('shows the LAN address and copy-invite when running', async () => {
    running.mockReturnValue(true);
    connectivity.mockResolvedValue({
      lan_addresses: ['192.168.1.5'],
      port: 25565,
      online_mode: true,
    });
    render(ServerConnectView, { serverId: 'srv-1' });
    expect(await screen.findByText('192.168.1.5:25565')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Copy invite' })).toBeTruthy();
  });

  it('explains online-mode ON', async () => {
    running.mockReturnValue(true);
    connectivity.mockResolvedValue({
      lan_addresses: ['192.168.1.5'],
      port: 25565,
      online_mode: true,
    });
    render(ServerConnectView, { serverId: 'srv-1' });
    expect(await screen.findByText(/only friends signed in with a Microsoft/i)).toBeTruthy();
  });
});
