import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  __resetMemoryBoundsCache,
  FALLBACK_MEMORY_BOUNDS,
  loadMemoryBounds,
} from '$lib/instances/memory-bounds';

const { mockBounds } = vi.hoisted(() => ({ mockBounds: vi.fn() }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    instanceMemoryBounds: mockBounds,
  },
}));

const REAL_BOUNDS = {
  min_mb: 1024,
  max_mb: 32768,
  default_mb: 6144,
  recommended_max_mb: 24576,
  step_mb: 256,
  ram_known: true,
};

afterEach(() => {
  __resetMemoryBoundsCache();
  mockBounds.mockReset();
});

describe('loadMemoryBounds', () => {
  it('returns the backend bounds', async () => {
    mockBounds.mockResolvedValue(REAL_BOUNDS);

    expect(await loadMemoryBounds()).toEqual(REAL_BOUNDS);
  });

  it('caches a successful result — the command runs once across calls', async () => {
    mockBounds.mockResolvedValue(REAL_BOUNDS);

    await loadMemoryBounds();
    await loadMemoryBounds();

    expect(mockBounds).toHaveBeenCalledTimes(1);
  });

  it('falls back without poisoning the cache, so a later call can recover', async () => {
    mockBounds.mockRejectedValueOnce(new Error('ram probe failed'));

    // First call degrades to the static fallback…
    expect(await loadMemoryBounds()).toEqual(FALLBACK_MEMORY_BOUNDS);

    // …and the next call re-fetches rather than serving a cached fallback.
    mockBounds.mockResolvedValue(REAL_BOUNDS);
    expect(await loadMemoryBounds()).toEqual(REAL_BOUNDS);
    expect(mockBounds).toHaveBeenCalledTimes(2);
  });
});
