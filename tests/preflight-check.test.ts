/**
 * `createPreflight().check` — the FRESH pre-flight the Play gate and «Fix all» run. It hands the
 * raw result back and commits it like any reload, so the Overview, the Installed panel and the
 * gate never show different verdicts.
 */
import { flushSync } from 'svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const m = vi.hoisted(() => ({ instanceDependencyPreflight: vi.fn() }));
const cache = vi.hoisted(() => ({ get: vi.fn(), set: vi.fn(), delete: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({ commands: m }));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));
vi.mock('$lib/mods/preflight-cache', () => ({ preflightCache: cache }));

import { createPreflight } from '$lib/mods/preflight.svelte';

const seeded = { violations: [] };
const fresh = { violations: [{ dependent_sha1: 'a' } as never] };
const stale = { violations: [{ dependent_sha1: 'old' } as never] };

/** A command answer the test releases by hand. */
function deferred<T>() {
  let resolve: (v: T) => void = () => {};
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

describe('createPreflight().check', () => {
  beforeEach(() => {
    m.instanceDependencyPreflight.mockReset();
    cache.set.mockReset();
    cache.get.mockReturnValue(seeded); // seeded: the mount effect runs no command
    m.instanceDependencyPreflight.mockResolvedValue({ status: 'ok', data: fresh });
  });

  it('runs a fresh pre-flight, commits it and hands back the raw result', async () => {
    const p = createPreflight(() => 'i1');
    flushSync();
    expect(await p.check('i1')).toEqual({ status: 'ok', data: fresh });
    expect(p.report).toEqual(fresh);
    expect(cache.set).toHaveBeenCalledWith('i1', fresh);
    p.dispose();
  });

  it('never lands a verdict on a profile that is no longer active', async () => {
    const p = createPreflight(() => 'i2');
    flushSync();
    await p.check('i1');
    expect(p.report).toEqual(seeded);
    p.dispose();
  });

  it('a failed check keeps the last report and says so', async () => {
    m.instanceDependencyPreflight.mockResolvedValue({ status: 'error', error: 'io' });
    const p = createPreflight(() => 'i1');
    flushSync();
    expect(await p.check('i1')).toEqual({ status: 'error', error: 'io' });
    expect(p.report).toEqual(seeded);
    expect(p.error).toBe('io');
    p.dispose();
  });

  it('an older load that answers last never overwrites a newer one', async () => {
    // A mod event's reload is still out when Play runs its check: the check saw the later mods
    // folder, so the reload's answer is stale however late it lands.
    const reload = deferred<unknown>();
    const check = deferred<unknown>();
    m.instanceDependencyPreflight
      .mockReturnValueOnce(reload.promise)
      .mockReturnValueOnce(check.promise);
    const p = createPreflight(() => 'i1');
    flushSync();
    p.reload();
    const checked = p.check('i1');
    check.resolve({ status: 'ok', data: fresh });
    await checked;
    reload.resolve({ status: 'ok', data: stale });
    await new Promise<void>((r) => setTimeout(r, 0));
    expect(p.report).toEqual(fresh);
    expect(cache.set.mock.calls).toEqual([['i1', fresh]]);
    p.dispose();
  });
});
