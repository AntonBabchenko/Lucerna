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
 * In the app the signal is Tauri's window focus event — the OS activating the window. The page's
 * own `focus` event cannot be relied on there: WebView2 keeps the page focused while another app is
 * in front (`document.hasFocus()` stays true), so coming back fires nothing. The DOM event stays as
 * the signal outside Tauri; when both arrive for one return, `cb` runs once.
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
