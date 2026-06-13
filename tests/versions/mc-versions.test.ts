import { describe, expect, it, vi } from 'vitest';
import type { VersionEntry } from '$lib/ipc/bindings';

// Declared before the SUT import so vitest hoists the mock ahead of the module
// graph. listVersions returns the tauri-specta `{ status, data | error }` shape.
const listVersionsMock = vi.fn();
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    listVersions: (...a: unknown[]) => listVersionsMock(...a),
  },
}));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));

// Intentionally the REAL shared rune (not mocked) so the tests verify the actual
// publish-to-mcVersions path on a successful load.
import { mcVersions } from '$lib/settings/state.svelte';
import { createMcVersions } from '$lib/versions/mc-versions.svelte';

function entry(id: string): VersionEntry {
  return {
    id,
    version_type: 'release',
    release_date: '2023-01-01T00:00:00+00:00',
    url: `https://example.com/${id}.json`,
  };
}

describe('createMcVersions self-heal', () => {
  it('load() success populates value, clears error, publishes to the shared rune', async () => {
    listVersionsMock.mockReset();
    mcVersions.value = [];
    listVersionsMock.mockResolvedValue({ status: 'ok', data: [entry('1.20.1')] });
    const m = createMcVersions();
    await m.load();
    expect(m.value.map((v) => v.id)).toEqual(['1.20.1']);
    expect(m.error).toBeNull();
    expect(mcVersions.value.map((v) => v.id)).toEqual(['1.20.1']);
    m.dispose();
  });

  it('load() failure sets error and a bounded backoff retry recovers', async () => {
    vi.useFakeTimers();
    listVersionsMock.mockReset();
    mcVersions.value = [];
    listVersionsMock
      .mockResolvedValueOnce({ status: 'error', error: 'net down' })
      .mockResolvedValue({ status: 'ok', data: [entry('1.20.1')] });
    const m = createMcVersions();
    await m.load();
    expect(m.error).toBe('net down');
    expect(listVersionsMock).toHaveBeenCalledTimes(1);
    // First backoff step is 2000ms.
    await vi.advanceTimersByTimeAsync(2000);
    expect(listVersionsMock).toHaveBeenCalledTimes(2);
    expect(m.error).toBeNull();
    expect(m.value.map((v) => v.id)).toEqual(['1.20.1']);
    m.dispose();
    vi.useRealTimers();
  });

  it('refetches on the window "online" event while in the error state', async () => {
    listVersionsMock.mockReset();
    mcVersions.value = [];
    listVersionsMock.mockResolvedValueOnce({ status: 'error', error: 'net down' });
    const m = createMcVersions();
    await m.load();
    expect(m.error).toBe('net down');
    listVersionsMock.mockResolvedValue({ status: 'ok', data: [entry('1.20.1')] });
    window.dispatchEvent(new Event('online'));
    // onOnline → load() → attempt() → await listVersions() is a 2-await chain, so
    // flush via a macrotask (drains the whole pending microtask queue). A single
    // `await Promise.resolve()` tick would be insufficient here.
    await new Promise((r) => setTimeout(r, 0));
    expect(m.error).toBeNull();
    expect(m.value.map((v) => v.id)).toEqual(['1.20.1']);
    m.dispose();
  });

  it('ignores the "online" event when healthy (no error)', async () => {
    listVersionsMock.mockReset();
    mcVersions.value = [];
    listVersionsMock.mockResolvedValue({ status: 'ok', data: [entry('1.20.1')] });
    const m = createMcVersions();
    await m.load();
    expect(listVersionsMock).toHaveBeenCalledTimes(1);
    window.dispatchEvent(new Event('online'));
    await new Promise((r) => setTimeout(r, 0));
    expect(listVersionsMock).toHaveBeenCalledTimes(1);
    m.dispose();
  });

  it('dispose() removes the online listener and cancels pending backoff', async () => {
    vi.useFakeTimers();
    listVersionsMock.mockReset();
    mcVersions.value = [];
    listVersionsMock.mockResolvedValue({ status: 'error', error: 'net down' });
    const m = createMcVersions();
    await m.load();
    expect(listVersionsMock).toHaveBeenCalledTimes(1);
    m.dispose();
    await vi.advanceTimersByTimeAsync(2000); // pending backoff must not fire
    expect(listVersionsMock).toHaveBeenCalledTimes(1);
    window.dispatchEvent(new Event('online')); // listener must be gone
    await Promise.resolve();
    expect(listVersionsMock).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });

  it('drops a superseded load: a newer load() wins over a slow stale one', async () => {
    listVersionsMock.mockReset();
    mcVersions.value = [];
    let releaseFirst: (v: unknown) => void = () => {};
    const firstPending = new Promise((res) => {
      releaseFirst = res;
    });
    listVersionsMock
      .mockImplementationOnce(() => firstPending)
      .mockResolvedValue({ status: 'ok', data: [entry('1.20.1')] });
    const m = createMcVersions();
    const a = m.load(); // parks on firstPending
    await new Promise((r) => setTimeout(r, 0));
    const b = m.load(); // supersedes A
    await b; // B resolves ok
    releaseFirst({ status: 'error', error: 'stale' }); // A would set error...
    await a; // ...but A is stale → dropped by the seq guard
    expect(m.error).toBeNull();
    expect(m.value.map((v) => v.id)).toEqual(['1.20.1']);
    m.dispose();
  });

  it('stops auto-retrying after the bounded backoff is exhausted', async () => {
    vi.useFakeTimers();
    listVersionsMock.mockReset();
    mcVersions.value = [];
    listVersionsMock.mockResolvedValue({ status: 'error', error: 'net down' });
    const m = createMcVersions();
    await m.load(); // attempt 1 (initial)
    expect(listVersionsMock).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(2000); // attempt 2 (backoff step 0)
    await vi.advanceTimersByTimeAsync(5000); // attempt 3 (backoff step 1)
    await vi.advanceTimersByTimeAsync(10000); // attempt 4 (backoff step 2)
    expect(listVersionsMock).toHaveBeenCalledTimes(4); // 1 initial + 3 bounded retries
    // Schedule exhausted — no further timer fires no matter how long we wait.
    await vi.advanceTimersByTimeAsync(60000);
    expect(listVersionsMock).toHaveBeenCalledTimes(4);
    m.dispose();
    vi.useRealTimers();
  });
});
