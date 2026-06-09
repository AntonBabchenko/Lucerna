/**
 * Human-readable byte size. Mirrors the original local helper from
 * ImportPickerDialog: B / KiB / MiB with one decimal; empty string for
 * a null/non-positive size (so callers can render "" without a guard).
 */
export function formatSize(size: number | null): string {
  if (size == null || size <= 0) return '';
  if (size < 1024) return `${Math.round(size)} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KiB`;
  return `${(size / 1024 / 1024).toFixed(1)} MiB`;
}
