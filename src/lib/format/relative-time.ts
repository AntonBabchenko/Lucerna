import type { Translate } from '$lib/i18n';

/**
 * Render a unix-ms timestamp as a short relative phrase
 * ("12s ago", "5m ago", "3h ago", "2d ago"). `null` / `undefined` / 0 →
 * the locale's "never" string. Caller passes `$t` so the result is
 * reactive on locale change.
 */
export function relativeTime(t: Translate, ms: number | null | undefined): string {
  if (!ms) return t('format.relativeTime.never');
  const diff = Date.now() - ms;
  const sec = Math.floor(diff / 1000);
  if (sec < 60) return t('format.relativeTime.secondsAgo', { n: sec });
  const min = Math.floor(sec / 60);
  if (min < 60) return t('format.relativeTime.minutesAgo', { n: min });
  const hr = Math.floor(min / 60);
  if (hr < 24) return t('format.relativeTime.hoursAgo', { n: hr });
  const days = Math.floor(hr / 24);
  return t('format.relativeTime.daysAgo', { n: days });
}

/**
 * Render a unix-ms timestamp as a coarse relative date phrase
 * (today / yesterday / Nd ago / Nmo ago / Ny ago).
 * `null` → empty string (a sentinel the caller checks before rendering a
 * surrounding separator — deliberately not an i18n key, so it stays empty
 * in every locale without a parity-guard exception).
 * Caller passes `$t` so the result is reactive on locale change.
 */
export function relativeDate(t: Translate, ms: number | null): string {
  if (ms == null) return '';
  const diff = Date.now() - ms;
  const day = 1000 * 60 * 60 * 24;
  if (diff < day) return t('format.relativeDate.today');
  if (diff < 2 * day) return t('format.relativeDate.yesterday');
  const days = Math.floor(diff / day);
  if (days < 30) return t('format.relativeDate.daysAgo', { n: days });
  const months = Math.floor(days / 30);
  if (months < 12) return t('format.relativeDate.monthsAgo', { n: months });
  return t('format.relativeDate.yearsAgo', { n: Math.floor(months / 12) });
}
