import { beforeEach, describe, expect, it } from 'vitest';
import {
  __resetTasksForTest,
  cancelQueued,
  clearFinished,
  finish,
  start,
  taskFor,
  taskList,
  upsertProgress,
} from '$lib/tasks/registry.svelte';

const base = { title: 't', scope: {}, phase: null, progress: null };

describe('task registry', () => {
  beforeEach(() => __resetTasksForTest());

  it('runs one serial task and queues the rest', () => {
    start({ ...base, id: 'a', kind: 'verify', lane: 'serial' });
    start({ ...base, id: 'b', kind: 'repair', lane: 'serial' });
    expect(taskList().find((t) => t.id === 'a')?.state).toBe('running');
    expect(taskList().find((t) => t.id === 'b')?.state).toBe('queued');
  });

  it('never queues a concurrent task behind a serial one', () => {
    start({ ...base, id: 'a', kind: 'verify', lane: 'serial' });
    start({ ...base, id: 'g', kind: 'game-install', lane: 'concurrent' });
    expect(taskList().find((t) => t.id === 'g')?.state).toBe('running');
  });

  it('offers cancel only while queued', () => {
    start({ ...base, id: 'a', kind: 'verify', lane: 'serial' });
    start({ ...base, id: 'b', kind: 'repair', lane: 'serial' });
    expect(taskList().find((t) => t.id === 'a')?.caps.cancellable).toBe(false);
    expect(taskList().find((t) => t.id === 'b')?.caps.cancellable).toBe(true);
  });

  it('never offers cancel or reorder for a modal task', () => {
    start({ ...base, id: 'm', kind: 'data-migration', lane: 'modal' });
    expect(taskList().find((t) => t.id === 'm')?.caps).toEqual({
      cancellable: false,
      reorderable: false,
    });
  });

  it('promotes the next serial task when the running one finishes', () => {
    start({ ...base, id: 'a', kind: 'verify', lane: 'serial' });
    start({ ...base, id: 'b', kind: 'repair', lane: 'serial' });
    finish('a', { state: 'ok' });
    expect(taskList().find((t) => t.id === 'b')?.state).toBe('running');
  });

  it('drops a queued task on cancel without touching the running one', () => {
    start({ ...base, id: 'a', kind: 'verify', lane: 'serial' });
    start({ ...base, id: 'b', kind: 'repair', lane: 'serial' });
    cancelQueued('b');
    expect(taskList().map((t) => t.id)).toEqual(['a']);
  });

  it('keeps details for the ten most recent finished tasks only', () => {
    for (let i = 0; i < 12; i++) {
      start({ ...base, id: `t${i}`, kind: 'mod-install', lane: 'concurrent' });
      finish(`t${i}`, { state: 'ok', details: [{ name: `${i}.jar` } as never] });
    }
    const withDetails = taskList().filter((t) => t.details !== null);
    expect(withDetails).toHaveLength(10);
    expect(withDetails.some((t) => t.id === 't11')).toBe(true);
    expect(withDetails.some((t) => t.id === 't0')).toBe(false);
  });

  it('looks a live task up by scope and forgets it once finished', () => {
    start({ ...base, id: 'v', kind: 'verify', lane: 'serial', scope: { instanceId: 'inst-1' } });
    expect(taskFor({ instanceId: 'inst-1' })?.id).toBe('v');
    expect(taskFor({ instanceId: 'other' })).toBeNull();
    finish('v', { state: 'ok' });
    expect(taskFor({ instanceId: 'inst-1' })).toBeNull();
  });

  it('ignores a progress write for an unknown id', () => {
    expect(() => upsertProgress('nope', { phase: 'x' })).not.toThrow();
  });

  it('clears finished tasks without touching active ones', () => {
    start({ ...base, id: 'a', kind: 'verify', lane: 'serial' });
    start({ ...base, id: 'b', kind: 'repair', lane: 'serial' });
    finish('a', { state: 'ok' });
    clearFinished();
    expect(taskList().map((t) => t.id)).toEqual(['b']);
  });

  it('leaves the queue and running task alone when nothing has finished yet', () => {
    start({ ...base, id: 'a', kind: 'verify', lane: 'serial' });
    clearFinished();
    expect(taskList().map((t) => t.id)).toEqual(['a']);
  });

  it('does not resurrect a cleared task’s details bookkeeping on a later finish', () => {
    // Regression guard for finishedOrder: clearing finished tasks must also
    // drop their ids from the eviction-order bookkeeping, or a later finish
    // could evict a phantom id and silently under-evict a real one.
    start({ ...base, id: 'a', kind: 'mod-install', lane: 'concurrent' });
    finish('a', { state: 'ok', details: [{ name: 'a.jar' } as never] });
    clearFinished();

    start({ ...base, id: 'b', kind: 'mod-install', lane: 'concurrent' });
    finish('b', { state: 'ok', details: [{ name: 'b.jar' } as never] });
    expect(taskList().find((t) => t.id === 'b')?.details).not.toBeNull();
  });
});
