import { afterEach, describe, expect, it, vi } from 'vitest';

const tauri = vi.hoisted(() => ({
  handler: null as ((e: { payload: boolean }) => void) | null,
  unlisten: vi.fn(),
  resolveListen: null as ((u: () => void) => void) | null,
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    onFocusChanged: (h: (e: { payload: boolean }) => void) => {
      tauri.handler = h;
      return new Promise<() => void>((r) => (tauri.resolveListen = r));
    },
  }),
}));

import { onWindowFocus } from '$lib/ui/window-focus';

const flush = () => new Promise((r) => setTimeout(r, 0));
// The runtime's own flag, which the module reads to tell it is inside Tauri.
const setInTauri = (v: boolean) => {
  (globalThis as { isTauri?: boolean }).isTauri = v || undefined;
};

afterEach(() => {
  setInTauri(false);
  tauri.handler = null;
  tauri.resolveListen = null;
  tauri.unlisten.mockReset();
  vi.useRealTimers();
});

describe('onWindowFocus', () => {
  it('outside Tauri, calls back on each window focus until unsubscribed', () => {
    vi.useFakeTimers();
    const cb = vi.fn();
    const off = onWindowFocus(cb);
    window.dispatchEvent(new Event('focus'));
    vi.advanceTimersByTime(1000);
    window.dispatchEvent(new Event('focus'));
    off();
    vi.advanceTimersByTime(1000);
    window.dispatchEvent(new Event('focus'));
    expect(cb).toHaveBeenCalledTimes(2);
  });

  // WebView2 keeps the page focused while another app is in front, so the page's own event does
  // not come on return: the window's focus event is the one that must work.
  it('in Tauri, calls back when the window gains focus, not when it loses it', async () => {
    setInTauri(true);
    const cb = vi.fn();
    const off = onWindowFocus(cb);
    tauri.resolveListen?.(tauri.unlisten);
    await flush();
    tauri.handler?.({ payload: false });
    expect(cb).not.toHaveBeenCalled();
    tauri.handler?.({ payload: true });
    expect(cb).toHaveBeenCalledTimes(1);
    off();
    expect(tauri.unlisten).toHaveBeenCalledTimes(1);
  });

  it('one return heard twice (window event and page event) calls back once', async () => {
    setInTauri(true);
    const cb = vi.fn();
    onWindowFocus(cb);
    tauri.resolveListen?.(tauri.unlisten);
    await flush();
    tauri.handler?.({ payload: true });
    window.dispatchEvent(new Event('focus'));
    expect(cb).toHaveBeenCalledTimes(1);
  });

  it('unsubscribed before the window listener is in place, drops it as soon as it is', async () => {
    setInTauri(true);
    const off = onWindowFocus(vi.fn());
    off();
    tauri.resolveListen?.(tauri.unlisten);
    await flush();
    expect(tauri.unlisten).toHaveBeenCalledTimes(1);
  });
});
