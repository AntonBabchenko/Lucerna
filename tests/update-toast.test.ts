import { describe, it, expect, beforeEach } from 'vitest';
import { pushActionToast, toastList, dismiss } from '$lib/toasts/toasts.svelte';

describe('action toast', () => {
  beforeEach(() => {
    for (const t of [...toastList()]) dismiss(t.id);
  });

  it('stores an action with label and run callback', () => {
    let ran = 0;
    const id = pushActionToast('info', 'Update available', { label: 'Update', run: () => ran++ });
    const t = toastList().find((x) => x.id === id)!;
    expect(t.action?.label).toBe('Update');
    t.action!.run();
    expect(ran).toBe(1);
  });

  it('action toast is sticky (info/warning do not auto-dismiss)', () => {
    const id = pushActionToast('warning', 'Verify failed', { label: 'Open', run: () => {} });
    expect(toastList().some((x) => x.id === id)).toBe(true);
  });
});
