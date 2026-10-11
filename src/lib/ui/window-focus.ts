/**
 * Call `cb` each time the launcher window gains focus: the user coming back from the file manager
 * the Add-ons folder button opened, with packs dropped in by hand. Returns the unsubscribe.
 */
export function onWindowFocus(cb: () => void): () => void {
  // The window's own focus only: an element taking focus inside the page is not a return.
  const onFocus = (e: Event) => {
    if (e.target === window) cb();
  };
  window.addEventListener('focus', onFocus);
  return () => window.removeEventListener('focus', onFocus);
}
