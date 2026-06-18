import { describe, expect, it, vi } from 'vitest';
import type { WorldQuickEntry } from '$lib/ipc/bindings';

const listWorldNamesMock = vi.fn();
let exitHandler: (() => void) | null = null;
const unlistenMock = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    listWorldNames: (...a: unknown[]) => listWorldNamesMock(...a),
  },
  events: {
    processExited: {
      listen: (cb: () => void) => {
        exitHandler = cb;
        return Promise.resolve(unlistenMock);
      },
    },
  },
}));

import { createQuickWorlds } from '$lib/worlds/quick-worlds.svelte';

function entry(folder_name: string, ms: number): WorldQuickEntry {
  return { folder_name, modified_unix_ms: ms };
}
const flush = () => new Promise((r) => setTimeout(r, 0));

describe('createQuickWorlds', () => {
  it('load() populates worlds for the instance', async () => {
    listWorldNamesMock.mockReset();
    listWorldNamesMock.mockResolvedValue({ status: 'ok', data: [entry('A', 2), entry('B', 1)] });
    const q = createQuickWorlds();
    q.load('i1');
    await flush();
    expect(listWorldNamesMock).toHaveBeenCalledWith('i1');
    expect(q.worlds.map((w) => w.folder_name)).toEqual(['A', 'B']);
    q.dispose();
  });

  it('swallows an error result into an empty list', async () => {
    listWorldNamesMock.mockReset();
    listWorldNamesMock.mockResolvedValue({ status: 'error', error: 'io' });
    const q = createQuickWorlds();
    q.load('i1');
    await flush();
    expect(q.worlds).toEqual([]);
    q.dispose();
  });

  it('clear() empties the list and drops an in-flight load', async () => {
    listWorldNamesMock.mockReset();
    let release: (v: unknown) => void = () => {};
    listWorldNamesMock.mockImplementationOnce(
      () =>
        new Promise((res) => {
          release = res;
        }),
    );
    const q = createQuickWorlds();
    q.load('i1'); // parks
    q.clear(); // supersedes
    release({ status: 'ok', data: [entry('A', 1)] });
    await flush();
    expect(q.worlds).toEqual([]);
    q.dispose();
  });

  it('reloads the current instance when the game exits', async () => {
    listWorldNamesMock.mockReset();
    listWorldNamesMock.mockResolvedValue({ status: 'ok', data: [entry('A', 1)] });
    const q = createQuickWorlds();
    q.load('i1');
    await flush();
    listWorldNamesMock.mockResolvedValue({
      status: 'ok',
      data: [entry('A', 1), entry('NewWorld', 3)],
    });
    exitHandler?.(); // simulate processExited
    await flush();
    expect(q.worlds.map((w) => w.folder_name)).toContain('NewWorld');
    q.dispose();
  });

  it('dispose() before the listen promise resolves still tears the listener down', async () => {
    unlistenMock.mockReset();
    const q = createQuickWorlds();
    // dispose synchronously, before the (already-resolved) listen promise's
    // .then handler runs — the early-dispose branch must call unlisten itself.
    q.dispose();
    await flush();
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });
});
