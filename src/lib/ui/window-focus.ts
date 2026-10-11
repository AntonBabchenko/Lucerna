/**
 * Call `cb` each time the launcher window gains focus: the user coming back from the file manager
 * the Add-ons folder button opened, with packs dropped in by hand. Returns the unsubscribe.
 */
export function onWindowFocus(cb: () => void): () => void {
  window.addEventListener('focus', cb);
  return () => window.removeEventListener('focus', cb);
}
