import { describe, expect, it, vi } from 'vitest';

import { dispatchIntent } from '$lib/launch/intent';

describe('dispatchIntent', () => {
  it('routes an open_url intent to the URL handler only', async () => {
    const onUrl = vi.fn();
    const onLaunch = vi.fn();
    await dispatchIntent(
      { kind: 'open_url', url: 'lucerna://modpack?source=modrinth&project=x' },
      { onUrl, onLaunch },
    );
    expect(onUrl).toHaveBeenCalledWith('lucerna://modpack?source=modrinth&project=x');
    expect(onLaunch).not.toHaveBeenCalled();
  });

  it('routes a launch intent with its quick-play target', async () => {
    const onUrl = vi.fn();
    const onLaunch = vi.fn();
    await dispatchIntent(
      { kind: 'launch', instance: 'p', quick_play: { kind: 'multiplayer', address: 'h:1' } },
      { onUrl, onLaunch },
    );
    expect(onLaunch).toHaveBeenCalledWith('p', { kind: 'multiplayer', address: 'h:1' });
    expect(onUrl).not.toHaveBeenCalled();
  });

  it('passes a null quick-play through for a plain instance launch', async () => {
    const onLaunch = vi.fn();
    await dispatchIntent(
      { kind: 'launch', instance: 'p', quick_play: null },
      { onUrl: vi.fn(), onLaunch },
    );
    expect(onLaunch).toHaveBeenCalledWith('p', null);
  });

  it('awaits an async handler before resolving', async () => {
    const order: string[] = [];
    await dispatchIntent(
      { kind: 'open_url', url: 'lucerna://modpack?source=modrinth&project=x' },
      {
        onUrl: async () => {
          await Promise.resolve();
          order.push('handler');
        },
        onLaunch: vi.fn(),
      },
    );
    order.push('after');
    expect(order).toEqual(['handler', 'after']);
  });

  it('does nothing when there is no pending intent', async () => {
    const onUrl = vi.fn();
    const onLaunch = vi.fn();
    await dispatchIntent(null, { onUrl, onLaunch });
    expect(onUrl).not.toHaveBeenCalled();
    expect(onLaunch).not.toHaveBeenCalled();
  });
});
