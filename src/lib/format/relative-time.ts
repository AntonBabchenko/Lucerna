/**
 * Render a unix-ms timestamp as a short relative phrase
 * ("12s ago", "5m ago", "3h ago", "2d ago"). `null` / `undefined` → "never".
 */
export function relativeTime(ms: number | null | undefined): string {
  if (!ms) return 'never';
  const diff = Date.now() - ms;
  const sec = Math.floor(diff / 1000);
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const days = Math.floor(hr / 24);
  return `${days}d ago`;
}
