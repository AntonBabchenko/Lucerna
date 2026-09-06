// The Play menu's world list after a migration: `+page.svelte`'s
// `refreshQuickWorlds` (which the Worlds tab calls through `onWorldsChanged`)
// is the same `load(id)` the selection effect performs, so a world moved out
// of the active instance leaves the menu instead of Quick-Playing a folder
// that is gone (world-migration spec §7 "Completion", §9 "Frontend").
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
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

// The composable above is only half the §9 line: it can refresh, but nothing
// makes it. `+page.svelte` is the whole app shell and is not renderable under
// vitest (it is excluded from the coverage denominator for that reason), so
// its half is a source scan — the `tests/page-silent-failure-guards.test.ts`
// shape. MainTabs' link in the same chain is pinned in tests/main-tabs.test.ts.
const PAGE = resolve('src/routes/+page.svelte');
const pageSrc = readFileSync(PAGE, 'utf8');

/**
 * The body of a function, brace-matched from its header. Adequate here for the
 * same reason as in `page-silent-failure-guards`: the body scanned holds no
 * brace inside a string or a template literal. If one ever does, this returns a
 * short slice and the assertions below fail loudly instead of passing.
 */
function functionBody(header: string): string {
  const start = pageSrc.indexOf(header);
  expect(start, `${header} must still exist in +page.svelte`).toBeGreaterThan(-1);
  const open = pageSrc.indexOf('{', start);
  let depth = 0;
  for (let i = open; i < pageSrc.length; i++) {
    if (pageSrc[i] === '{') depth++;
    else if (pageSrc[i] === '}' && --depth === 0) return pageSrc.slice(open, i + 1);
  }
  throw new Error(`unbalanced braces after ${header}`);
}

describe('+page.svelte — the Play menu refresh the Worlds tab triggers', () => {
  it('hands `refreshQuickWorlds` to MainTabs as onWorldsChanged', () => {
    // Without this forward the migration outcome never reaches the Play menu
    // and "Play this world" keeps offering a folder that was moved away.
    expect(pageSrc).toContain('onWorldsChanged={refreshQuickWorlds}');
  });

  it('refreshes through the same load/clear rule that fills the menu', () => {
    const body = functionBody('function refreshQuickWorlds()');
    // Both halves matter: `load` re-reads the eligible instance's worlds, and
    // `clear` is what an ineligible one must resolve to — a refresh that only
    // ever loaded would leave a stale list standing.
    expect(body).toContain('quickWorlds.load(');
    expect(body).toContain('quickWorlds.clear(');
  });
});
