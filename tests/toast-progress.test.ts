import { beforeEach, describe, expect, it } from 'vitest';
import { dismiss, pushProgress, toastList, updateToastProgress } from '$lib/toasts/toasts.svelte';

describe('toast progress', () => {
  beforeEach(() => {
    for (const t of [...toastList()]) dismiss(t.id);
  });

  it('pushProgress creates an info toast that starts indeterminate (progress null)', () => {
    const id = pushProgress('Downloading update…');
    const t = toastList().find((x) => x.id === id);
    expect(t?.kind).toBe('info');
    expect(t?.progress).toBeNull();
  });

  it('updateToastProgress sets a fractional value on the matching toast', () => {
    const id = pushProgress('Downloading update…');
    updateToastProgress(id, 0.42);
    expect(toastList().find((x) => x.id === id)?.progress).toBe(0.42);
  });

  it('updateToastProgress on an unknown id is a no-op', () => {
    const id = pushProgress('Downloading update…');
    updateToastProgress(9999, 0.9);
    expect(toastList().find((x) => x.id === id)?.progress).toBeNull();
  });
});
