import { afterEach, describe, expect, it, vi } from 'vitest';
import { attachPopoverDismiss } from '$lib/ui/popover-dismiss';

describe('attachPopoverDismiss', () => {
  let cleanup: (() => void) | undefined;

  afterEach(() => {
    cleanup?.();
    cleanup = undefined;
    document.body.innerHTML = '';
  });

  it('dismisses on an outside scroll and on resize', () => {
    const onDismiss = vi.fn();
    cleanup = attachPopoverDismiss({ onDismiss });
    window.dispatchEvent(new Event('scroll'));
    expect(onDismiss).toHaveBeenCalledTimes(1);
    window.dispatchEvent(new Event('resize'));
    expect(onDismiss).toHaveBeenCalledTimes(2);
  });

  it('ignores scrolls originating inside ignoreScrollWithin', () => {
    const list = document.createElement('div');
    const child = document.createElement('div');
    list.appendChild(child);
    document.body.appendChild(list);

    const onDismiss = vi.fn();
    cleanup = attachPopoverDismiss({ onDismiss, ignoreScrollWithin: () => list });

    // Capture-phase window listener sees a scroll dispatched on a descendant of
    // the list; contains() → true → ignored.
    child.dispatchEvent(new Event('scroll'));
    expect(onDismiss).not.toHaveBeenCalled();

    // A scroll from outside the list still dismisses.
    const outside = document.createElement('div');
    document.body.appendChild(outside);
    outside.dispatchEvent(new Event('scroll'));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('detaches every listener on cleanup', () => {
    const onDismiss = vi.fn();
    const detach = attachPopoverDismiss({ onDismiss });
    detach();
    window.dispatchEvent(new Event('scroll'));
    window.dispatchEvent(new Event('resize'));
    expect(onDismiss).not.toHaveBeenCalled();
  });
});
