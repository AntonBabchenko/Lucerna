/**
 * Call `cb` each time the launcher window gains focus: the user coming back from the file manager
 * the Add-ons folder button opened, with packs dropped in by hand. Returns the unsubscribe.
 * (`focus` does not bubble: a listener on `window` hears the window's own focus, not an element's.)
 */
export function onWindowFocus(cb: () => void): () => void {
  window.addEventListener('focus', cb);
  return () => window.removeEventListener('focus', cb);
}
