import { beforeEach, describe, expect, it, vi } from 'vitest';

const instanceIcon = vi.fn();
vi.mock('$lib/ipc/bindings', () => ({
  commands: { instanceIcon: (id: string) => instanceIcon(id) },
}));

import {
  __clearInstanceIconCache,
  invalidateInstanceIcon,
  loadInstanceIcon,
} from '$lib/instances/instance-icon-cache';

describe('instance-icon-cache', () => {
  beforeEach(() => {
    __clearInstanceIconCache();
    instanceIcon.mockReset();
  });

  it('maps an ok payload to a png data URL', async () => {
    instanceIcon.mockResolvedValue({ status: 'ok', data: { png_base64: 'AAAA' } });
    expect(await loadInstanceIcon('i1')).toBe('data:image/png;base64,AAAA');
  });

  it('returns null when the instance has no icon', async () => {
    instanceIcon.mockResolvedValue({ status: 'ok', data: null });
    expect(await loadInstanceIcon('i2')).toBeNull();
  });

  it('dedupes concurrent loads into one IPC call', async () => {
    instanceIcon.mockResolvedValue({ status: 'ok', data: { png_base64: 'AAAA' } });
    await Promise.all([loadInstanceIcon('i3'), loadInstanceIcon('i3')]);
    expect(instanceIcon).toHaveBeenCalledTimes(1);
  });

  it('re-fetches after invalidateInstanceIcon', async () => {
    instanceIcon.mockResolvedValue({ status: 'ok', data: { png_base64: 'AAAA' } });
    await loadInstanceIcon('i4');
    invalidateInstanceIcon('i4');
    await loadInstanceIcon('i4');
    expect(instanceIcon).toHaveBeenCalledTimes(2);
  });
});
