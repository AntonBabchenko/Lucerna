import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ModpackInstanceUpdate } from '$lib/ipc/bindings';

// Mock the IPC layer before importing the store.
const checkMock = vi.fn();
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modpacksCheckUpdates: (...args: unknown[]) => checkMock(...args),
  },
}));

import { modpackUpdates } from '$lib/modpacks/modpack-updates.svelte';

function ok(data: ModpackInstanceUpdate[]) {
  return { status: 'ok' as const, data };
}

afterEach(() => {
  checkMock.mockReset();
  modpackUpdates.reset();
});

describe('modpackUpdates store', () => {
  it('populates statuses and derives updateCount from a sweep', async () => {
    checkMock.mockResolvedValue(
      ok([
        {
          instance_id: 'a',
          status: {
            kind: 'update_available',
            entry: {
              id: 'v2',
              name: 'P',
              version_number: '2.0',
              game_versions: [],
              loaders: [],
              date_published: '',
            },
          },
        },
        { instance_id: 'b', status: { kind: 'up_to_date' } },
      ]),
    );
    await modpackUpdates.sweep(['a', 'b'], { force: true });
    expect(modpackUpdates.statusFor('a')?.kind).toBe('update_available');
    expect(modpackUpdates.hasUpdate('a')).toBe(true);
    expect(modpackUpdates.hasUpdate('b')).toBe(false);
    expect(modpackUpdates.updateCount).toBe(1);
  });

  it('skips a non-forced sweep within the TTL window', async () => {
    checkMock.mockResolvedValue(ok([{ instance_id: 'a', status: { kind: 'up_to_date' } }]));
    await modpackUpdates.sweep(['a'], { force: true });
    await modpackUpdates.sweep(['a']); // within TTL, not forced
    expect(checkMock).toHaveBeenCalledTimes(1);
  });

  it('force bypasses the TTL', async () => {
    checkMock.mockResolvedValue(ok([{ instance_id: 'a', status: { kind: 'up_to_date' } }]));
    await modpackUpdates.sweep(['a'], { force: true });
    await modpackUpdates.sweep(['a'], { force: true });
    expect(checkMock).toHaveBeenCalledTimes(2);
  });

  it('invalidate drops an entry from the count', async () => {
    checkMock.mockResolvedValue(
      ok([
        {
          instance_id: 'a',
          status: {
            kind: 'update_available',
            entry: {
              id: 'v2',
              name: 'P',
              version_number: '2.0',
              game_versions: [],
              loaders: [],
              date_published: '',
            },
          },
        },
      ]),
    );
    await modpackUpdates.sweep(['a'], { force: true });
    expect(modpackUpdates.updateCount).toBe(1);
    modpackUpdates.invalidate('a');
    expect(modpackUpdates.updateCount).toBe(0);
    expect(modpackUpdates.statusFor('a')).toBeUndefined();
  });

  it('swallows an IPC error without throwing', async () => {
    checkMock.mockResolvedValue({ status: 'error', error: { kind: 'ModsNetwork' } });
    await expect(modpackUpdates.sweep(['a'], { force: true })).resolves.toBeUndefined();
    expect(modpackUpdates.updateCount).toBe(0);
  });
});
