import { getCurrentWindow } from '@tauri-apps/api/window';

/** Two signals of one return land within this; the second is the same return. */
const SAME_RETURN_MS = 300;

/** Running inside Tauri — the flag `isTauri()` of `@tauri-apps/api/core` reads. Read here, not
 *  imported: many tests stub that module wholesale, and this runs under every Installed view. */
const inTauri = (): boolean => Boolean((globalThis as { isTauri?: unknown }).isTauri);

/**
 * Call `cb` each time the launcher window gains focus: the user coming back from the file manager
 * the Add-ons folder button opened, with packs dropped in by hand. Returns the unsubscribe.
 *
 * Two signals are heard. Tauri's window focus event is the OS activating the window, on every
 * platform the launcher ships for. The page's own `focus` is what a webview chooses to raise: on
 * Windows it comes with every activation (measured 2026-10-12, about 10 ms before Tauri's), but
 * `document.hasFocus()` there stayed true with another app in front, so the page's view of focus is
 * not the OS's — and outside Tauri (tests, a plain browser) it is the only signal. One return heard
 * by both runs `cb` once.
 */
export function onWindowFocus(cb: () => void): () => void {
  let last = Number.NEGATIVE_INFINITY;
  const fire = () => {
    const now = Date.now();
    if (now - last < SAME_RETURN_MS) return;
    last = now;
    cb();
  };
  window.addEventListener('focus', fire);

  let disposed = false;
  let unlistenWindow: (() => void) | null = null;
  if (inTauri()) {
    getCurrentWindow()
      .onFocusChanged(({ payload: focused }) => {
        if (focused) fire();
      })
      .then((unlisten) => {
        // Unsubscribed before the listener was in place: drop it at once.
        if (disposed) unlisten();
        else unlistenWindow = unlisten;
      })
      .catch((e: unknown) => {
        console.warn('[window-focus] could not listen for the window gaining focus:', e);
      });
  }
  return () => {
    disposed = true;
    window.removeEventListener('focus', fire);
    unlistenWindow?.();
  };
}
