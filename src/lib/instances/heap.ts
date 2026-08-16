/** Pure display/derivation helpers for the per-instance memory slider. */

import type { Translate } from '$lib/i18n';

const MB_PER_GB = 1024;

/**
 * Human label for a heap size: GB with one decimal at ≥ 1 GB, else whole MB.
 *
 * Takes `t` for the same reason `$lib/format/size.ts` does — the unit AND the
 * decimal separator are language-dependent, and this label is read aloud by
 * screen readers through the slider's `aria-valuetext`. The RAW number goes to
 * `t()`; the dictionary's `{n, number, …}` argument owns the rounding, so
 * Russian renders "8,0 ГБ" instead of a hardcoded English "8.0 GB".
 *
 * Deliberately NOT `formatSize`: that helper takes BYTES and its GB bucket
 * carries two decimals, whereas a heap is chosen in whole MB steps and the
 * slider's endpoint labels have to stay short.
 */
export function formatHeapLabel(t: Translate, mb: number): string {
  if (mb >= MB_PER_GB) return t('format.heap.gigabytes', { n: mb / MB_PER_GB });
  return t('format.heap.megabytes', { n: mb });
}

/** Whether a chosen heap exceeds the safe threshold (only when RAM is known). */
export function isAboveRecommended(
  valueMb: number,
  recommendedMaxMb: number,
  ramKnown: boolean,
): boolean {
  return ramKnown && valueMb > recommendedMaxMb;
}
