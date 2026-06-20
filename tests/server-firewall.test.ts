import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';

const { connectivity, running, firewallStatus, firewallAddRule } = vi.hoisted(() => ({
  connectivity: vi.fn(),
  running: vi.fn(),
  firewallStatus: vi.fn(),
  firewallAddRule: vi.fn(),
}));

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: { connectivity, running, firewallStatus, firewallAddRule },
}));

// navigator.clipboard is not available in jsdom.
Object.defineProperty(globalThis.navigator, 'clipboard', {
  value: { writeText: vi.fn().mockResolvedValue(undefined) },
  configurable: true,
});

import ServerConnectView from '$lib/servers/ServerConnectView.svelte';

describe('ServerConnectView firewall banner', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    connectivity.mockReset();
    running.mockReset();
    firewallStatus.mockReset();
    firewallAddRule.mockReset();
    // Default: connectivity resolves with a minimal snapshot so the view renders.
    connectivity.mockResolvedValue({
      lan_addresses: ['192.168.1.5'],
      port: 25565,
      online_mode: true,
    });
  });

  it('shows the "Add firewall rule" button when needs_rule', async () => {
    running.mockReturnValue(true);
    firewallStatus.mockResolvedValue('needs_rule');
    firewallAddRule.mockResolvedValue({ ok: true });

    render(ServerConnectView, { serverId: 'srv-1' });

    expect(await screen.findByText('Add firewall rule')).toBeTruthy();
    expect(await screen.findByText('Windows Firewall may be blocking friends')).toBeTruthy();
  });

  it('calls firewallAddRule(serverId) when the button is clicked', async () => {
    running.mockReturnValue(true);
    firewallStatus.mockResolvedValue('needs_rule');
    firewallAddRule.mockResolvedValue({ ok: true });

    render(ServerConnectView, { serverId: 'srv-1' });

    const btn = await screen.findByRole('button', { name: 'Add firewall rule' });
    fireEvent.click(btn);

    // Give the async handler a tick to fire.
    await new Promise((r) => setTimeout(r, 0));

    expect(firewallAddRule).toHaveBeenCalledWith('srv-1');
  });

  it('does NOT show the "Add firewall rule" button when allowed', async () => {
    running.mockReturnValue(true);
    firewallStatus.mockResolvedValue('allowed');

    render(ServerConnectView, { serverId: 'srv-1' });

    // Wait for any async rendering to settle.
    await screen.findByText(/On your network/i);
    expect(screen.queryByRole('button', { name: 'Add firewall rule' })).toBeNull();
  });

  it('shows no firewall banner at all when not_applicable', async () => {
    running.mockReturnValue(true);
    firewallStatus.mockResolvedValue('not_applicable');

    render(ServerConnectView, { serverId: 'srv-1' });

    await screen.findByText(/On your network/i);
    expect(screen.queryByText('Windows Firewall may be blocking friends')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Add firewall rule' })).toBeNull();
  });

  it('shows no firewall banner when the server is stopped', async () => {
    running.mockReturnValue(false);
    firewallStatus.mockResolvedValue('needs_rule');

    render(ServerConnectView, { serverId: 'srv-1' });

    expect(await screen.findByText('Start the server to get a join address.')).toBeTruthy();
    expect(screen.queryByText('Windows Firewall may be blocking friends')).toBeNull();
  });
});
