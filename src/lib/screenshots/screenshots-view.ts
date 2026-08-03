// src/lib/screenshots/screenshots-view.ts
//
// Pure view logic shared by both screenshot surfaces: ordering, calendar
// grouping and group captions. No DOM, no IPC — unit-tested in isolation;
// ScreenshotBrowser renders its output. Mirrors journal-view.ts.

import type { Screenshot } from '$lib/ipc/bindings';

export type SortDir = 'newest' | 'oldest';

/**
 * A screenshot's timestamp as a plain number.
 *
 * specta exports every Rust `f64` as `number | null` (a non-finite float
 * serialises to `null`), so the nullability is a wire artefact rather than a
 * real state — a listed file has an mtime unless the filesystem refused one,
 * in which case 0 sorts it to the tail rather than hiding it.
 */
export function shotTime(shot: Screenshot): number {
  return shot.modified_unix_ms ?? 0;
}

/** Order by capture time. Returns a copy; the caller's array is never mutated. */
export function sortShots(shots: Screenshot[], dir: SortDir): Screenshot[] {
  const sign = dir === 'newest' ? -1 : 1;
  return [...shots].sort((a, b) => sign * (shotTime(a) - shotTime(b)));
}
