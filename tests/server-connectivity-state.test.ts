import { beforeEach, describe, expect, it, vi } from 'vitest';

const { listen, connectivity, list } = vi.hoisted(() => ({
  listen: vi.fn().mockResolvedValue(() => {}),
  connectivity: vi.fn(),
  list: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: { serverConnectivity: connectivity, serverList: list },
  events: {
    serverLogLine: { listen },
    serverSpawned: { listen },
    serverExited: { listen },
    serverUploadProgress: { listen },
  },
}));

import { serverState } from '$lib/servers/server-state.svelte';

describe('serverState.connectivity', () => {
  beforeEach(() => connectivity.mockReset());
  it('returns the connectivity snapshot on ok', async () => {
    connectivity.mockResolvedValue({
      status: 'ok',
      data: { lan_addresses: ['192.168.1.5'], port: 25565, online_mode: true },
    });
    const r = await serverState.connectivity('srv-1');
    expect(connectivity).toHaveBeenCalledWith('srv-1');
    expect(r?.lan_addresses).toEqual(['192.168.1.5']);
    expect(r?.port).toBe(25565);
  });
  it('returns null on error', async () => {
    connectivity.mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: 'p', details: 'd' },
    });
    expect(await serverState.connectivity('srv-1')).toBeNull();
  });
});
