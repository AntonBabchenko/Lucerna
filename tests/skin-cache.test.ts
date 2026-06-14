import { afterEach, describe, expect, test, vi } from 'vitest';

const accountSkin = vi.fn();
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    accountSkin: (uuid: string) => accountSkin(uuid),
  },
}));

import { __clearSkinCache, loadSkinHead } from '../src/lib/accounts/skin-cache';

afterEach(() => {
  __clearSkinCache();
  accountSkin.mockReset();
});

describe('loadSkinHead', () => {
  test('returns the png base64 on an ok result', async () => {
    accountSkin.mockResolvedValue({
      status: 'ok',
      data: { uuid: 'u', texture_url: 't', skin_png_base64: 'AAAA' },
    });
    expect(await loadSkinHead('u')).toBe('AAAA');
  });

  test('returns null when the account has no skin (ok + null data)', async () => {
    accountSkin.mockResolvedValue({ status: 'ok', data: null });
    expect(await loadSkinHead('u')).toBeNull();
  });

  test('returns null on an error result', async () => {
    accountSkin.mockResolvedValue({ status: 'error', error: 'boom' });
    expect(await loadSkinHead('u')).toBeNull();
  });

  test('dedups concurrent + repeat calls for the same uuid', async () => {
    accountSkin.mockResolvedValue({
      status: 'ok',
      data: { uuid: 'u', texture_url: 't', skin_png_base64: 'AAAA' },
    });
    await Promise.all([loadSkinHead('u'), loadSkinHead('u')]);
    await loadSkinHead('u');
    expect(accountSkin).toHaveBeenCalledTimes(1);
  });

  test('evicts the cache entry on a rejected call so a later call retries', async () => {
    accountSkin.mockRejectedValueOnce(new Error('bridge down'));
    expect(await loadSkinHead('u')).toBeNull();
    // After a rejection the entry is evicted; the next call hits the command again.
    accountSkin.mockResolvedValue({
      status: 'ok',
      data: { uuid: 'u', texture_url: 't', skin_png_base64: 'BBBB' },
    });
    expect(await loadSkinHead('u')).toBe('BBBB');
    expect(accountSkin).toHaveBeenCalledTimes(2);
  });
});
