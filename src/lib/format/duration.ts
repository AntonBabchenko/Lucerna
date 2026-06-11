import type { Translate } from '$lib/i18n';

/**
 * Render a seconds count as a short localized "Xh Ym" or "Ym" string.
 * Below 1 minute, returns the locale's "< 1m" string.
 * Null / undefined coerce to 0.
 * Caller passes `$t` so the result is reactive on locale change.
 */
export function formatDuration(t: Translate, seconds: number | null | undefined): string {
  const s = seconds ?? 0;
  if (s < 60) return t('format.duration.lessThanMinute');
  const hours = Math.floor(s / 3600);
  const minutes = Math.floor((s % 3600) / 60);
  if (hours === 0) return t('format.duration.minutes', { m: minutes });
  return t('format.duration.hoursMinutes', { h: hours, m: minutes });
}
