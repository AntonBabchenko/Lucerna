import type { Translate } from '$lib/i18n';

/**
 * Human-readable byte size using B / KB / MB / GB (1024 divisor).
 * Returns an empty string for null / non-positive sizes so callers can
 * render "" without a guard. Caller passes `$t` so the unit label
 * localizes on locale change.
 */
export function formatSize(t: Translate, size: number | null | undefined): string {
  if (size == null || size <= 0) return '';
  if (size < 1024) return t('format.size.bytes', { n: Math.round(size) });
  if (size < 1024 * 1024) return t('format.size.kilobytes', { n: (size / 1024).toFixed(1) });
  if (size < 1024 * 1024 * 1024)
    return t('format.size.megabytes', { n: (size / 1048576).toFixed(1) });
  return t('format.size.gigabytes', { n: (size / 1073741824).toFixed(2) });
}
