import { describe, expect, it, vi } from 'vitest';
import { onWindowFocus } from '$lib/ui/window-focus';

describe('onWindowFocus', () => {
  it('calls back on every window focus until unsubscribed', () => {
    const cb = vi.fn();
    const off = onWindowFocus(cb);
    window.dispatchEvent(new Event('focus'));
    window.dispatchEvent(new Event('focus'));
    off();
    window.dispatchEvent(new Event('focus'));
    expect(cb).toHaveBeenCalledTimes(2);
  });
});
