/**
 * Render a seconds count as a short "Xh Ym" or "Ym" string.
 * Below 1 minute, returns "< 1m". Null / undefined coerce to 0.
 */
export function formatDuration(seconds: number | null | undefined): string {
  const s = seconds ?? 0;
  if (s < 60) return '< 1m';
  const hours = Math.floor(s / 3600);
  const minutes = Math.floor((s % 3600) / 60);
  if (hours === 0) return `${minutes}m`;
  return `${hours}h ${minutes}m`;
}
