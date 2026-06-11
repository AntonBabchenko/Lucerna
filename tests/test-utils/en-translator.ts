/**
 * Minimal English translator stub for unit tests that need to call
 * localized formatter functions (formatSize, formatDuration, relativeTime, etc.)
 * outside a Svelte/svelte-i18n context.
 *
 * It does a simple `{placeholder}` substitution using the English locale
 * strings defined inline. Only keys used by the format/ module are listed
 * here — add more as needed.
 */

import type { Translate } from '$lib/i18n';

const EN_STRINGS: Record<string, string> = {
  'format.relativeTime.never': 'never',
  'format.relativeTime.secondsAgo': '{n}s ago',
  'format.relativeTime.minutesAgo': '{n}m ago',
  'format.relativeTime.hoursAgo': '{n}h ago',
  'format.relativeTime.daysAgo': '{n}d ago',
  'format.relativeDate.empty': '',
  'format.relativeDate.today': 'today',
  'format.relativeDate.yesterday': 'yesterday',
  'format.relativeDate.daysAgo': '{n}d ago',
  'format.relativeDate.monthsAgo': '{n}mo ago',
  'format.relativeDate.yearsAgo': '{n}y ago',
  'format.size.bytes': '{n} B',
  'format.size.kilobytes': '{n} KB',
  'format.size.megabytes': '{n} MB',
  'format.size.gigabytes': '{n} GB',
  'format.duration.lessThanMinute': '< 1m',
  'format.duration.minutes': '{m}m',
  'format.duration.hoursMinutes': '{h}h {m}m',
  'format.unit.megabyte': 'MB',
};

function interpolate(template: string, values?: Record<string, unknown>): string {
  if (!values) return template;
  return template.replace(/\{(\w+)\}/g, (_, key) => {
    const v = values[key];
    return v !== undefined && v !== null ? String(v) : `{${key}}`;
  });
}

export const enTr: Translate = (key, values) => {
  const tmpl = EN_STRINGS[key] ?? key;
  return interpolate(tmpl, values as Record<string, unknown> | undefined);
};
