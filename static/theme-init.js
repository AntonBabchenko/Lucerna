// Sync .dark class on <html> before first paint based on the user's
// persisted theme preference. Falls through to the OS setting when
// pref === 'system' or no pref is stored. Wrapped in try/catch
// because private-mode browsers can throw on localStorage access.
//
// Loaded as a separate <script src> rather than inline so the
// production CSP can require script-src 'self'. The tag is
// synchronous (no defer/async) so this runs before first paint.
(function () {
  try {
    var pref = localStorage.getItem('theme') || 'system';
    var dark =
      pref === 'dark' ||
      (pref === 'system' &&
        window.matchMedia('(prefers-color-scheme: dark)').matches);
    if (dark) document.documentElement.classList.add('dark');
  } catch (_) {}
})();
