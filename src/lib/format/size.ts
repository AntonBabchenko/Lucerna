import type { Translate } from '$lib/i18n';

/**
 * Human-readable byte size using B / KB / MB / GB (1024 divisor).
 * Returns an empty string for null / non-positive sizes so callers can
 * render "" without a guard. Caller passes `$t` so the unit label
 * localizes on locale change.
 *
 * Hands `t()` the RAW number, never a pre-formatted string. The
 * dictionary's `{n, number, ::.0 group-off}` ICU argument owns both the
 * rounding (one or two fraction digits, per unit) and the decimal
 * separator, so the same call renders "1.5 KB" in English and "1,5 КБ"
 * in Russian.
 */
export function formatSize(t: Translate, size: number | null | undefined): string {
  if (size == null || size <= 0) return '';
  if (size < 1024) return t('format.size.bytes', { n: Math.round(size) });
  if (size < 1024 * 1024) return t('format.size.kilobytes', { n: size / 1024 });
  if (size < 1024 * 1024 * 1024) return t('format.size.megabytes', { n: size / 1048576 });
  return t('format.size.gigabytes', { n: size / 1073741824 });
}
