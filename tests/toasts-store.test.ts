import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  dismiss,
  pushInfo,
  pushProgress,
  pushSuccess,
  pushWarning,
  SUCCESS_TTL_MS,
  toastList,
  updateToast,
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

  it('pushInfo adds an info toast with lines and never auto-dismisses', () => {
    pushInfo('Microsoft sign-in pending approval', ['Awaiting admin approval']);
    const list = toastList();
    expect(list).toHaveLength(1);
    expect(list[0].kind).toBe('info');
    expect(list[0].title).toBe('Microsoft sign-in pending approval');
    expect(list[0].lines).toEqual(['Awaiting admin approval']);
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

  it('patches an existing toast in place, keeping its slot', () => {
    const first = pushProgress('Importing modpack');
    pushInfo('unrelated');

    updateToast(first, { kind: 'success', title: 'Modpack imported' });

    const list = toastList();
    expect(list[0].id).toBe(first);
    expect(list[0].kind).toBe('success');
    expect(list[0].title).toBe('Modpack imported');
  });

  it('ignores a patch for an unknown id', () => {
    expect(() => updateToast(9999, { title: 'x' })).not.toThrow();
  });
});
