import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({ getDataLocation: vi.fn(), listen: vi.fn(), pushInfo: vi.fn() }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { getDataLocation: h.getDataLocation },
  events: { dataMigrationProgress: { listen: h.listen } },
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({ pushInfo: h.pushInfo }));

import DataMoveHost from '$lib/settings/DataMoveHost.svelte';

// Its own file on purpose: the store is a module singleton and this case needs one whose FIRST
// read fails (data-move-host.test.ts always seeds it with a successful read before mounting).
const FINAL = {
  kind: 'restart_required',
  old_root: 'C:\Old',
  new_root: 'D:\New',
  leftovers: [],
  old_root_intact: false,
  old_root_is_default: false,
  retry_possible: false,
};
const ok = (relocation: unknown) => ({
  status: 'ok',
  data: {
    effective: 'D:\New',
    configured: 'D:\New',
    fell_back: false,
    default_dir: 'C:\Default',
    relocation,
  },
});

beforeEach(() => {
  vi.clearAllMocks();
  h.listen.mockResolvedValue(() => {});
});

describe('DataMoveHost — the first status read failed', () => {
  it('keeps asking until a read succeeds, so a finished move still gets its Restart dialog', async () => {
    vi.useFakeTimers();
    try {
      // The page was reloaded after a move that ended in `restart_required`; the first read
      // fails. "Could not tell" must not settle as "no move" for the rest of the session.
      h.getDataLocation.mockRejectedValueOnce(new Error('ipc channel closed'));
      h.getDataLocation.mockResolvedValue(ok(FINAL));
      render(DataMoveHost);
      await vi.advanceTimersByTimeAsync(0);
      expect(h.getDataLocation).toHaveBeenCalledTimes(1);
      expect(screen.queryByRole('dialog')).toBeNull();

      await vi.advanceTimersByTimeAsync(3100);
      expect(h.getDataLocation).toHaveBeenCalledTimes(2);
      expect(screen.queryByRole('button', { name: 'Restart' })).not.toBeNull();

      // …and it stops asking once it knows.
      await vi.advanceTimersByTimeAsync(10_000);
      expect(h.getDataLocation).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });
});
