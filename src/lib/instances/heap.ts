/** Pure display/derivation helpers for the per-instance memory slider. */

const MB_PER_GB = 1024;

/** Human label for a heap size: GB with one decimal at ≥ 1 GB, else raw MB. */
export function formatHeapLabel(mb: number): string {
  if (mb >= MB_PER_GB) {
    return `${(mb / MB_PER_GB).toFixed(1)} GB`;
  }
  return `${mb} MB`;
}

/** Whether a chosen heap exceeds the safe threshold (only when RAM is known). */
export function isAboveRecommended(
  valueMb: number,
  recommendedMaxMb: number,
  ramKnown: boolean,
): boolean {
  return ramKnown && valueMb > recommendedMaxMb;
}
