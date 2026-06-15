import type { LogFileMeta } from '$lib/ipc/bindings';

/** Logs that auto-cleanup / "clear old" must never remove. Mirrors the
 *  Rust `PROTECTED` list in `logs/retention.rs`. */
const PROTECTED = new Set(['latest.log', 'debug.log']);

export function isProtectedLog(name: string): boolean {
  return PROTECTED.has(name);
}

export interface ClearOldPreview {
  count: number;
  bytes: number;
}

/** How many files "Clear old" would delete and how many bytes that frees,
 *  computed from the already-loaded file list (no backend round-trip). */
export function clearOldPreview(files: LogFileMeta[]): ClearOldPreview {
  let count = 0;
  let bytes = 0;
  for (const f of files) {
    if (isProtectedLog(f.name)) continue;
    count += 1;
    bytes += Math.max(0, f.size_bytes ?? 0);
  }
  return { count, bytes };
}
