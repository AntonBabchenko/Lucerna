import { beforeEach, describe, expect, it } from 'vitest';
import {
  __resetCfgQueueForTest,
  cfgBusy,
  cfgSuperseded,
  enqueueCfgChange,
} from '$lib/instances/settings/cfg-queue.svelte';

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

const settle = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => __resetCfgQueueForTest());

describe('the per-instance config queue', () => {
  it('runs one change per instance at a time and keeps only the newest queued one', async () => {
    const first = deferred();
    const ran: string[] = [];
    const discarded: string[] = [];
    void enqueueCfgChange('a', async () => {
      ran.push('fabric');
      await first.promise;
    });
    void enqueueCfgChange(
      'a',
      async () => void ran.push('quilt'),
      () => discarded.push('quilt'),
    );
    void enqueueCfgChange('a', async () => void ran.push('forge'));
    expect(discarded).toEqual(['quilt']);
    expect(cfgSuperseded('a')).toBe(true);
    first.resolve();
    await settle();
    expect(ran).toEqual(['fabric', 'forge']);
  });

  it('a second caller — a remounted section — joins the change already in flight', async () => {
    const first = deferred();
    const order: string[] = [];
    void enqueueCfgChange('a', async () => {
      await first.promise;
      order.push('first');
    });
    void enqueueCfgChange('a', async () => void order.push('second'));
    expect(order).toEqual([]);
    first.resolve();
    await settle();
    expect(order).toEqual(['first', 'second']);
  });

  it('different instances do not wait on each other', async () => {
    const a = deferred();
    const ran: string[] = [];
    void enqueueCfgChange('a', async () => {
      await a.promise;
      ran.push('a');
    });
    await enqueueCfgChange('b', async () => void ran.push('b'));
    expect(ran).toEqual(['b']);
    a.resolve();
    await settle();
    expect(ran).toEqual(['b', 'a']);
  });

  it('reports busy while a change for the instance is in flight', async () => {
    const d = deferred();
    const done = enqueueCfgChange('a', () => d.promise);
    expect(cfgBusy('a')).toBe(true);
    expect(cfgBusy('b')).toBe(false);
    d.resolve();
    await done;
    expect(cfgBusy('a')).toBe(false);
    expect(cfgSuperseded('a')).toBe(false);
  });

  it('a throwing change does not wedge the one behind it', async () => {
    const ran: string[] = [];
    const gate = deferred();
    void enqueueCfgChange('a', async () => {
      await gate.promise;
      throw new Error('ipc');
    });
    void enqueueCfgChange('a', async () => void ran.push('next'));
    gate.resolve();
    await settle();
    expect(ran).toEqual(['next']);
    expect(cfgBusy('a')).toBe(false);
  });
});
