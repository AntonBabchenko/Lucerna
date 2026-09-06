// The Play menu's world list after a migration: `+page.svelte`'s
// `refreshQuickWorlds` (which the Worlds tab calls through `onWorldsChanged`)
// is the same `load(id)` the selection effect performs, so a world moved out
// of the active instance leaves the menu instead of Quick-Playing a folder
// that is gone (world-migration spec §7 "Completion", §9 "Frontend").
import { waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

const ipc = vi.hoisted(() => ({ listWorldNames: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({
  commands: { listWorldNames: ipc.listWorldNames },
  events: { processExited: { listen: vi.fn().mockResolvedValue(() => {}) } },
}));

import { createQuickWorlds } from '$lib/worlds/quick-worlds.svelte';

function entries(...names: string[]) {
  return names.map((folder_name, i) => ({ folder_name, modified_unix_ms: i }));
}

function names(qw: ReturnType<typeof createQuickWorlds>): string[] {
  return qw.worlds.map((w) => w.folder_name);
}

afterEach(() => vi.clearAllMocks());

describe('createQuickWorlds — refresh after a migration', () => {
  it('a second load(id) replaces the list, so a moved world leaves the Play menu', async () => {
    ipc.listWorldNames.mockResolvedValueOnce({
      status: 'ok',
      data: entries('My World', 'Other World'),
    });
    const qw = createQuickWorlds();
    qw.load('src');
    await waitFor(() => expect(names(qw)).toEqual(['My World', 'Other World']));

    // What +page.svelte's `onWorldsChanged` does once WorldsTab reports a
    // landed migration: the same load(id) the selection effect performs.
    ipc.listWorldNames.mockResolvedValueOnce({ status: 'ok', data: entries('Other World') });
    qw.load('src');
    await waitFor(() => expect(names(qw)).toEqual(['Other World']));
    expect(ipc.listWorldNames).toHaveBeenCalledTimes(2);
    expect(ipc.listWorldNames).toHaveBeenNthCalledWith(2, 'src');
    qw.dispose();
  });

  it('a refresh that overtakes a still-pending earlier load wins', async () => {
    // The post-migration refresh can race the selection effect's own fetch;
    // the composable's `seq` guard must let the NEWER answer stand even when
    // the older request resolves later.
    let resolveFirst: (v: unknown) => void = () => {};
    ipc.listWorldNames.mockImplementationOnce(
      () =>
        new Promise((res) => {
          resolveFirst = res;
        }),
    );
    ipc.listWorldNames.mockResolvedValueOnce({ status: 'ok', data: entries('Other World') });
    const qw = createQuickWorlds();
    qw.load('src');
    qw.load('src');
    await waitFor(() => expect(names(qw)).toEqual(['Other World']));

    resolveFirst({ status: 'ok', data: entries('My World', 'Other World') });
    await new Promise((r) => setTimeout(r, 0));
    expect(names(qw)).toEqual(['Other World']);
    qw.dispose();
  });
});
