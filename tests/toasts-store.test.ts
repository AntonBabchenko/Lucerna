import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  dismiss,
  pushSuccess,
  pushWarning,
  SUCCESS_TTL_MS,
  toastList,
} from '$lib/toasts/toasts.svelte';

// The store is module-global state shared across tests — clear it in
// beforeEach. Fake timers make the success auto-dismiss deterministic and
// stop a stray real timeout from a previous test firing mid-test.
describe('toasts store', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    for (const t of [...toastList()]) dismiss(t.id);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('pushSuccess adds a success toast with no detail lines', () => {
    pushSuccess('Imported Pack');
    const list = toastList();
    expect(list).toHaveLength(1);
    expect(list[0].kind).toBe('success');
    expect(list[0].title).toBe('Imported Pack');
    expect(list[0].lines).toEqual([]);
  });

  it('a success toast auto-dismisses after SUCCESS_TTL_MS', () => {
    pushSuccess('done');
    expect(toastList()).toHaveLength(1);
    vi.advanceTimersByTime(SUCCESS_TTL_MS);
    expect(toastList()).toHaveLength(0);
  });

  it('pushWarning adds a warning toast with lines and never auto-dismisses', () => {
    pushWarning('2 mods failed', ['a.jar', 'b.jar']);
    const list = toastList();
    expect(list).toHaveLength(1);
    expect(list[0].kind).toBe('warning');
    expect(list[0].lines).toEqual(['a.jar', 'b.jar']);
    vi.advanceTimersByTime(SUCCESS_TTL_MS * 10);
    expect(toastList()).toHaveLength(1);
  });

  it('dismiss removes only the toast with the given id', () => {
    const id1 = pushWarning('first');
    const id2 = pushWarning('second');
    expect(toastList()).toHaveLength(2);
    dismiss(id1);
    const list = toastList();
    expect(list).toHaveLength(1);
    expect(list[0].id).toBe(id2);
  });
});
